//! What every plugin's web page does to join its editor to its engine
//! ([design/hosts.md]): a `SharedArrayBuffer` link when the page is
//! cross-origin isolated, else none, so the editor runs without sound.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use js_sys::SharedArrayBuffer;
use silkscreen::EngineLink;
use silkscreen::bridge::NullLink;
use silkscreen::bridge::sab::SabMidiOut;
use wasm_bindgen::JsValue;

use crate::transport::{SabWords, sab_link};

pub use silkscreen::bridge::RING_CAPACITY;

/// Whether the page may make a `SharedArrayBuffer`: served with the COOP
/// and COEP headers.
pub fn cross_origin_isolated() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &"crossOriginIsolated".into())
        .is_ok_and(|v| v.is_truthy())
}

/// A page's link to its engine.
#[derive(Debug)]
pub struct Connection {
    /// The engine's buffer, to hand the worklet (`startAudio`); `None`
    /// off a cross-origin-isolated page.
    pub sab: Option<SharedArrayBuffer>,
    /// The MIDI ring's writer, for an on-screen keyboard.
    pub midi: Option<SabMidiOut<SabWords>>,
}

impl Connection {
    /// Send one MIDI 1.0 channel message to the engine. `false` if
    /// there's no engine link or the ring is full.
    pub fn midi(&mut self, status: u8, data1: u8, data2: u8) -> bool {
        self.midi
            .as_mut()
            .is_some_and(|m| m.send([status, data1, data2]))
    }
}

/// The editor's engine link and the page's side of it, the engine
/// starting from `initial` (every param's value).
pub fn connect(initial: &[f32]) -> Result<(Box<dyn EngineLink>, Connection), JsValue> {
    if !cross_origin_isolated() {
        let none = Connection {
            sab: None,
            midi: None,
        };
        return Ok((Box::new(NullLink), none));
    }
    let mut link = sab_link(initial, RING_CAPACITY).map_err(|e| e.to_string())?;
    let connection = Connection {
        sab: Some(link.words().buffer()),
        midi: link.midi_out(),
    };
    Ok((Box::new(link), connection))
}
