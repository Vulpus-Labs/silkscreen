//! The CLAP state extension: model values, keyed by stable CLAP id, the
//! editor's zoom ([design/hosts.md]), and the editor's UI state, such as each
//! page group's shown page ([design/faceplates.md]).
//!
//! The saved bytes are a **versioned contract**: projects saved by one
//! build are loaded by later ones. Change the layout only by bumping
//! [`VERSION`], and keep loading every earlier version.
//!
//! ```text
//! offset  size       field
//!      0     4       magic     b"SLKS"
//!      4     4       version   u32, little-endian
//!      8     4       count     u32, little-endian
//!     12     8×count entries   (id: u32, value: f32 bits), little-endian
//!      …     4       zoom      f32 bits, little-endian (version ≥ 2)
//!      …     4       ui count  u32, little-endian (version ≥ 3)
//!      …             ui pairs  (key, value), each a u16 little-endian byte
//!                              length then that many bytes of UTF-8
//! ```
//!
//! Values are plain. Version 1 has no zoom, and versions before 3 no UI
//! state. Bytes after the last field are ignored.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

use std::io::{Read, Write};

use clack_extensions::params::{HostParams, ParamRescanFlags};
use clack_extensions::state::PluginStateImpl;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use silkscreen::{HostEvent, ParamIndex, ParamSpec};
use silkscreen_host_native::nearest_zoom;

use crate::{MainThread, ParamIds, SilkscreenPlugin};

const MAGIC: [u8; 4] = *b"SLKS";

/// Layout version written by [`encode`].
pub const VERSION: u32 = 3;

const HEADER: usize = 12;
const ENTRY: usize = 8;

/// Why saved bytes could not be loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not silkscreen state at all.
    BadMagic,
    /// Saved by a newer build than this one.
    UnsupportedVersion(u32),
    /// Fewer bytes than the header promises.
    Truncated,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::BadMagic => write!(f, "not silkscreen state (bad magic)"),
            DecodeError::UnsupportedVersion(v) => write!(
                f,
                "state version {v} is newer than this build supports ({VERSION})"
            ),
            DecodeError::Truncated => write!(f, "state is truncated"),
        }
    }
}

/// What a saved state holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// Values for params this build knows; see [`decode`].
    pub values: Vec<(ParamIndex, f32)>,
    /// The editor zoom, snapped to a step. `None` before version 2.
    pub zoom: Option<f32>,
    /// The editor's UI state as (key, value) pairs. Empty before
    /// version 3.
    pub ui: Vec<(String, String)>,
}

/// Serialize `values` (one per spec, in table order) under each spec's id,
/// the editor's `zoom` and its `ui` state. A key or value longer than
/// 65535 bytes is cut short at a character boundary.
pub fn encode(
    specs: &[ParamSpec],
    values: impl IntoIterator<Item = f32>,
    zoom: f32,
    ui: &[(String, String)],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + ENTRY * specs.len() + 4);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(specs.len() as u32).to_le_bytes());
    for (spec, value) in specs.iter().zip(values) {
        out.extend_from_slice(&spec.id.to_le_bytes());
        out.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    out.extend_from_slice(&zoom.to_bits().to_le_bytes());
    out.extend_from_slice(&(ui.len() as u32).to_le_bytes());
    for (key, value) in ui {
        for text in [key, value] {
            let mut end = text.len().min(usize::from(u16::MAX));
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            out.extend_from_slice(&(end as u16).to_le_bytes());
            out.extend_from_slice(&text.as_bytes()[..end]);
        }
    }
    out
}

/// The UI pairs from `at`, or `Truncated` if they run past the end.
/// Text that isn't UTF-8 is replaced, not refused.
fn decode_ui(bytes: &[u8], mut at: usize) -> Result<Vec<(String, String)>, DecodeError> {
    fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], DecodeError> {
        let slice = bytes.get(*at..*at + n).ok_or(DecodeError::Truncated)?;
        *at += n;
        Ok(slice)
    }
    fn text(bytes: &[u8], at: &mut usize) -> Result<String, DecodeError> {
        let len = u16::from_le_bytes(take(bytes, at, 2)?.try_into().expect("two bytes"));
        Ok(String::from_utf8_lossy(take(bytes, at, usize::from(len))?).into_owned())
    }
    let count = u32::from_le_bytes(take(bytes, &mut at, 4)?.try_into().expect("four bytes"));
    let mut ui = Vec::new();
    for _ in 0..count {
        let key = text(bytes, &mut at)?;
        let value = text(bytes, &mut at)?;
        ui.push((key, value));
    }
    Ok(ui)
}

/// The saved values for params this build knows. Unknown ids are skipped;
/// params absent from `bytes` are absent from the result (the model gives
/// them their defaults).
pub fn decode(bytes: &[u8], ids: &ParamIds) -> Result<Saved, DecodeError> {
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if bytes.len() < 4 || bytes[..4] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    if bytes.len() < HEADER {
        return Err(DecodeError::Truncated);
    }
    let version = u32_at(4);
    if !(1..=VERSION).contains(&version) {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let count = u32_at(8) as usize;
    let trailer = if version >= 2 { 4 } else { 0 };
    let room = bytes.len() - HEADER;
    if room < trailer || (room - trailer) / ENTRY < count {
        return Err(DecodeError::Truncated);
    }
    let values = (0..count)
        .filter_map(|i| {
            let at = HEADER + i * ENTRY;
            let index = ids.index(ClapId::new(u32_at(at)))?;
            Some((index, f32::from_bits(u32_at(at + 4))))
        })
        .collect();
    let zoom = (version >= 2)
        .then(|| f32::from_bits(u32_at(HEADER + count * ENTRY)))
        .filter(|z| z.is_finite())
        .map(nearest_zoom);
    let ui = if version >= 3 {
        decode_ui(bytes, HEADER + count * ENTRY + 4)?
    } else {
        Vec::new()
    };
    Ok(Saved { values, zoom, ui })
}

impl<P: SilkscreenPlugin> PluginStateImpl for MainThread<'_, P> {
    fn save(&mut self, output: &mut OutputStream) -> Result<(), PluginError> {
        // Include anything the audio thread has heard from the host.
        self.pump();
        let state = self
            .state
            .try_borrow()
            .map_err(|_| PluginError::Message("editor state is busy"))?;
        let bytes = encode(
            P::UI.specs,
            state.param_values(),
            self.zoom,
            &state.ui_state(),
        );
        output
            .write_all(&bytes)
            .map_err(|_| PluginError::Message("could not write state"))
    }

    /// The new values reach the model as `HostEvent::StateLoaded`, and the
    /// engine through the bridge's load request (design/engine-bridge.md). The host
    /// is asked to rescan values, so its own view of them follows
    /// (design/hosts.md); it isn't sent them as edits: it chose this state.
    fn load(&mut self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut bytes = Vec::new();
        input
            .read_to_end(&mut bytes)
            .map_err(|_| PluginError::Message("could not read state"))?;
        let saved = decode(&bytes, &self.shared.ids).map_err(|e| {
            eprintln!("{}: {e}", P::ID);
            PluginError::Message("invalid state (details on stderr)")
        })?;
        // Fold in host changes from before the load first, so they can't
        // land on top of it.
        self.pump();
        {
            let mut state = self
                .state
                .try_borrow_mut()
                .map_err(|_| PluginError::Message("editor state is busy"))?;
            state.handle_host(HostEvent::StateLoaded(saved.values));
            state.restore_ui_state(&saved.ui);
        }
        let shared = self.host.shared();
        if let Some(params) = shared.get_extension::<HostParams>() {
            params.rescan(&mut self.host, ParamRescanFlags::VALUES);
        }
        if let Some(zoom) = saved.zoom {
            self.restore_zoom(zoom);
        }
        // While nothing is processing, a flush carries the load to the
        // engine end; activation also seeds from it.
        self.request_flush_if_needed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SPECS: [ParamSpec; 3] = [
        ParamSpec::new("A", 0.0, 1.0, 0.0).id(10),
        ParamSpec::new("B", -1.0, 1.0, 0.0).id(3),
        ParamSpec::new("C", 0.0, 100.0, 50.0).id(7),
    ];

    fn ids() -> ParamIds {
        ParamIds::new(&SPECS).unwrap()
    }

    #[test]
    fn every_value_round_trips_by_id() {
        let bytes = encode(&SPECS, [0.25, -0.5, 99.0], 1.5, &[]);
        assert_eq!(bytes.len(), HEADER + 3 * ENTRY + 4 + 4);
        let saved = decode(&bytes, &ids()).unwrap();
        assert_eq!(saved.zoom, Some(1.5));
        assert_eq!(
            saved.values,
            [
                (ParamIndex(0), 0.25),
                (ParamIndex(1), -0.5),
                (ParamIndex(2), 99.0)
            ]
        );

        // A later build that reordered its table still finds each value.
        let reordered = [SPECS[2], SPECS[0], SPECS[1]];
        let ids = ParamIds::new(&reordered).unwrap();
        let mut got = decode(&bytes, &ids).unwrap().values;
        got.sort_by_key(|(p, _)| *p);
        assert_eq!(
            got,
            [
                (ParamIndex(0), 99.0),
                (ParamIndex(1), 0.25),
                (ParamIndex(2), -0.5)
            ]
        );
    }

    #[test]
    fn unknown_ids_are_skipped_and_missing_ones_absent() {
        let old = [
            ParamSpec::new("Gone", 0.0, 1.0, 0.0).id(99),
            ParamSpec::new("B", -1.0, 1.0, 0.0).id(3),
        ];
        let bytes = encode(&old, [0.5, 0.75], 1.0, &[]);
        assert_eq!(
            decode(&bytes, &ids()).unwrap().values,
            [(ParamIndex(1), 0.75)]
        );
    }

    #[test]
    fn malformed_state_is_rejected() {
        let good = encode(&SPECS, [0.0, 0.0, 0.0], 1.0, &[]);
        assert_eq!(decode(b"", &ids()), Err(DecodeError::BadMagic));
        assert_eq!(decode(b"VST3xxxxxxxx", &ids()), Err(DecodeError::BadMagic));
        assert_eq!(decode(&good[..8], &ids()), Err(DecodeError::Truncated));
        assert_eq!(
            decode(&good[..good.len() - 1], &ids()),
            Err(DecodeError::Truncated)
        );

        let mut future = good.clone();
        future[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(
            decode(&future, &ids()),
            Err(DecodeError::UnsupportedVersion(4))
        );

        // A count that would overflow the buffer is caught before reading.
        let mut huge = good;
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&huge, &ids()), Err(DecodeError::Truncated));
    }

    #[test]
    fn version_1_loads_without_a_zoom() {
        let v3 = encode(&SPECS, [0.25, -0.5, 99.0], 2.0, &[]);
        let v2 = {
            let mut v = v3[..v3.len() - 4].to_vec();
            v[4..8].copy_from_slice(&2u32.to_le_bytes());
            v
        };
        let mut v1 = v2[..v2.len() - 4].to_vec();
        v1[4..8].copy_from_slice(&1u32.to_le_bytes());
        let saved = decode(&v1, &ids()).unwrap();
        assert_eq!(saved.zoom, None);
        assert_eq!(saved.values.len(), 3);
        // A v2 state missing its zoom is truncated, not v1.
        assert_eq!(
            decode(&v2[..v2.len() - 4], &ids()),
            Err(DecodeError::Truncated)
        );
    }

    #[test]
    fn a_saved_zoom_is_snapped_to_a_step() {
        let ids = ids();
        let zoom = |z: f32| {
            decode(&encode(&SPECS, [0.0; 3], z, &[]), &ids)
                .unwrap()
                .zoom
        };
        assert_eq!(zoom(1.3), Some(1.25));
        assert_eq!(zoom(f32::NAN), None);
        assert_eq!(zoom(f32::INFINITY), None);
    }

    #[test]
    fn ui_state_round_trips_from_version_3() {
        let ui = vec![
            ("page.fx".to_owned(), "2".to_owned()),
            ("note".to_owned(), "naïve ✓".to_owned()),
        ];
        let bytes = encode(&SPECS, [0.25, -0.5, 99.0], 1.0, &ui);
        let saved = decode(&bytes, &ids()).unwrap();
        assert_eq!(saved.ui, ui);
        assert_eq!(saved.values.len(), 3);
        assert_eq!(
            decode(&bytes[..bytes.len() - 1], &ids()),
            Err(DecodeError::Truncated)
        );
    }

    #[test]
    fn version_2_loads_without_ui_state() {
        let v3 = encode(&SPECS, [0.25, -0.5, 99.0], 1.5, &[]);
        let mut v2 = v3[..v3.len() - 4].to_vec();
        v2[4..8].copy_from_slice(&2u32.to_le_bytes());
        let saved = decode(&v2, &ids()).unwrap();
        assert_eq!(saved.zoom, Some(1.5));
        assert!(saved.ui.is_empty());
    }
}
