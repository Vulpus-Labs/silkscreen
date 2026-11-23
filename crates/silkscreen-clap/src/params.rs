//! The CLAP params extension over a `ParamSpec` table.

use std::ffi::CStr;
use std::fmt::Write as _;

use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::utils::Cookie;
use silkscreen::model::ParamKind;
use silkscreen::{ParamIndex, ParamSpec};

use crate::{MainThread, Processor, SilkscreenPlugin};

/// Stable CLAP ids ↔ dense param indices. Built once; lookups are a binary
/// search, so they're safe on the audio thread.
#[derive(Debug)]
pub struct ParamIds {
    /// `(clap id, index)`, sorted by id.
    by_id: Box<[(u32, u32)]>,
}

impl ParamIds {
    /// Fails if any spec lacks an id or two share one: hosts key projects on
    /// these, so they must be explicit and unique.
    pub fn new(specs: &[ParamSpec]) -> Result<Self, String> {
        let mut by_id = Vec::with_capacity(specs.len());
        for (i, s) in specs.iter().enumerate() {
            if s.id == ParamSpec::NO_ID {
                return Err(format!(
                    "param {i} (\"{}\") has no id. Give every ParamSpec a stable `.id(n)`; hosts save automation and projects against it",
                    s.name
                ));
            }
            by_id.push((s.id, i as u32));
        }
        by_id.sort_unstable();
        if let Some(w) = by_id.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(format!(
                "params \"{}\" and \"{}\" share id {}. Param ids must be unique",
                specs[w[0].1 as usize].name, specs[w[1].1 as usize].name, w[0].0
            ));
        }
        Ok(Self {
            by_id: by_id.into_boxed_slice(),
        })
    }

    pub fn index(&self, id: ClapId) -> Option<ParamIndex> {
        let id = id.get();
        self.by_id
            .binary_search_by_key(&id, |&(k, _)| k)
            .ok()
            .map(|pos| ParamIndex(self.by_id[pos].1))
    }
}

/// A host param-value event as `(index, clamped plain value)`, if it is one
/// for a param we know.
pub(crate) fn param_value(
    event: &UnknownEvent,
    ids: &ParamIds,
    specs: &[ParamSpec],
) -> Option<(ParamIndex, f32)> {
    let Some(CoreEventSpace::ParamValue(e)) = event.as_core_event() else {
        return None;
    };
    let index = ids.index(e.param_id()?)?;
    Some((index, specs[index.index()].clamp(e.value() as f32)))
}

/// CLAP flags for `spec`: discrete params step and list their labels;
/// patch state is hidden and not automatable (design/model.md).
fn param_flags(spec: &silkscreen::ParamSpec) -> ParamInfoFlags {
    let mut flags = match spec.kind {
        ParamKind::Continuous => ParamInfoFlags::empty(),
        ParamKind::Stepped { .. } | ParamKind::Toggle { .. } => {
            ParamInfoFlags::IS_STEPPED | ParamInfoFlags::IS_ENUM
        }
    };
    if spec.automatable {
        flags |= ParamInfoFlags::IS_AUTOMATABLE;
    } else {
        flags |= ParamInfoFlags::IS_HIDDEN;
    }
    flags
}

impl<P: SilkscreenPlugin> PluginMainThreadParams for MainThread<'_, P> {
    fn count(&mut self) -> u32 {
        P::UI.specs.len() as u32
    }

    fn get_info(&mut self, param_index: u32, info: &mut ParamInfoWriter) {
        let Some(spec) = P::UI.specs.get(param_index as usize) else {
            return;
        };
        let flags = param_flags(spec);
        info.set(&ParamInfo {
            id: ClapId::new(spec.id),
            flags,
            cookie: Cookie::default(),
            name: spec.name.as_bytes(),
            module: b"",
            min_value: f64::from(spec.min),
            max_value: f64::from(spec.max),
            default_value: f64::from(spec.default),
        });
    }

    fn get_value(&mut self, param_id: ClapId) -> Option<f64> {
        let index = self.shared.ids.index(param_id)?;
        // Fold in anything the audio thread has heard from the host.
        self.pump();
        let state = self.state.try_borrow().ok()?;
        Some(f64::from(state.model().get(index)))
    }

    fn value_to_text(
        &mut self,
        param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        let index = self.shared.ids.index(param_id).ok_or(std::fmt::Error)?;
        let spec = &P::UI.specs[index.index()];
        write!(writer, "{}", spec.format(value as f32))
    }

    fn text_to_value(&mut self, param_id: ClapId, text: &CStr) -> Option<f64> {
        let index = self.shared.ids.index(param_id)?;
        P::UI.specs[index.index()]
            .parse(text.to_str().ok()?)
            .map(f64::from)
    }

    /// Main-thread flush: only called while inactive, when this thread holds
    /// the link. UI edits go to the host as they would from `process`, and
    /// host values go through the engine end exactly as the audio thread's
    /// would, so the model hears them the same way.
    fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        if let Some(link) = self.link.as_mut() {
            // No engine while inactive: activation seeds it from the end.
            link.ui_to_host(P::UI.specs, output, |_| {});
            for event in input {
                if let Some((index, value)) = param_value(event, &self.shared.ids, P::UI.specs) {
                    link.end.publish_host(index, value);
                }
            }
        }
        self.pump();
    }
}

impl<P: SilkscreenPlugin> PluginAudioProcessorParams for Processor<'_, P> {
    fn flush(&mut self, input: &InputEvents, output: &mut OutputEvents) {
        Processor::flush(self, input, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ParamSpec = ParamSpec::new("A", 0.0, 1.0, 0.0).id(10);
    const B: ParamSpec = ParamSpec::new("B", 0.0, 1.0, 0.0).id(3);
    const C: ParamSpec = ParamSpec::new("C", 0.0, 1.0, 0.0).id(7);

    #[test]
    fn ids_are_stable_under_reordering() {
        let forward = ParamIds::new(&[A, B, C]).unwrap();
        let shuffled = ParamIds::new(&[C, A, B]).unwrap();
        // Each id finds its own spec's position in each table.
        assert_eq!(forward.index(ClapId::new(10)), Some(ParamIndex(0)));
        assert_eq!(shuffled.index(ClapId::new(10)), Some(ParamIndex(1)));
        assert_eq!(forward.index(ClapId::new(7)), Some(ParamIndex(2)));
        assert_eq!(shuffled.index(ClapId::new(7)), Some(ParamIndex(0)));
        assert_eq!(forward.index(ClapId::new(4)), None);
    }

    #[test]
    fn missing_ids_are_rejected_with_the_param_named() {
        let e = ParamIds::new(&[A, ParamSpec::new("Cutoff", 0.0, 1.0, 0.0)]).unwrap_err();
        assert!(e.contains("(\"Cutoff\") has no id"), "{e}");
        assert!(e.contains(".id(n)"), "{e}");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let e = ParamIds::new(&[A, B, ParamSpec::new("Other", 0.0, 1.0, 0.0).id(3)]).unwrap_err();
        assert!(e.contains("\"B\" and \"Other\" share id 3"), "{e}");
    }

    #[test]
    fn patch_state_is_hidden_and_not_automatable() {
        let depth = ParamSpec::new("Depth", -1.0, 1.0, 0.0).bipolar().id(1);
        let source = ParamSpec::stepped("Source", &["None", "Env", "LFO"], 0)
            .patch_state()
            .id(2);
        let d = param_flags(&depth);
        assert!(
            d.contains(ParamInfoFlags::IS_AUTOMATABLE) && !d.contains(ParamInfoFlags::IS_HIDDEN)
        );
        let s = param_flags(&source);
        assert!(!s.contains(ParamInfoFlags::IS_AUTOMATABLE));
        assert!(s.contains(ParamInfoFlags::IS_HIDDEN | ParamInfoFlags::IS_STEPPED));
    }
}
