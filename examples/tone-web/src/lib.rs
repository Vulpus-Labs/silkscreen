//! The tone example on a web page ([design/hosts.md]): the editor on a canvas,
//! the engine in an `AudioWorklet` (`tone-web-engine`), and notes from the
//! page's on-screen keyboard through the SAB's MIDI ring.
//!
//! `cargo silkscreen web tone-web --serve` builds and serves it.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use js_sys::SharedArrayBuffer;
use silkscreen_host_web::{Connection, WebHost, connect};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// The running page: the editor, the buffer to hand the worklet, and the
/// MIDI ring's writer for the keyboard.
#[wasm_bindgen]
#[derive(Debug)]
pub struct TonePage {
    host: WebHost,
    engine: Connection,
}

#[wasm_bindgen]
impl TonePage {
    /// The shared buffer for `startAudio`, or `undefined` if the page isn't
    /// cross-origin isolated.
    #[wasm_bindgen(getter)]
    pub fn sab(&self) -> Option<SharedArrayBuffer> {
        self.engine.sab.clone()
    }

    /// Send one MIDI 1.0 channel message to the engine. Returns `false` if
    /// there's no engine link or the ring is full.
    pub fn midi(&mut self, status: u8, data1: u8, data2: u8) -> bool {
        self.engine.midi(status, data1, data2)
    }

    /// Rects put on the canvas since mount, for diagnostics.
    pub fn puts(&self) -> u32 {
        self.host.puts()
    }
}

/// Mount the tone editor on `canvas` and start drawing.
#[wasm_bindgen]
pub fn start(canvas: HtmlCanvasElement) -> Result<TonePage, JsValue> {
    let initial: Vec<f32> = tone::PARAMS.iter().map(|p| p.default).collect();
    let (link, engine) = connect(&initial, tone::UI.frame)?;
    let host = silkscreen_host_web::mount(canvas, tone::editor(link))?;
    Ok(TonePage { host, engine })
}
