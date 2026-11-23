//! The gain example on a web page ([design/hosts.md]): the editor on a canvas
//! here, the engine in an `AudioWorklet` (`gain-web-engine`), joined by a
//! `SharedArrayBuffer`.
//!
//! `cargo silkscreen web gain-web --serve` builds both modules and serves the page
//! with the COOP and COEP headers a `SharedArrayBuffer` needs. Served
//! without them, the editor runs with no engine.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use js_sys::SharedArrayBuffer;
use silkscreen_host_web::{Connection, WebHost, connect};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// The running page: the editor, and the buffer to hand the worklet.
#[wasm_bindgen]
#[derive(Debug)]
pub struct GainPage {
    host: WebHost,
    engine: Connection,
}

#[wasm_bindgen]
impl GainPage {
    /// The shared buffer for `startAudio`, or `undefined` if the page isn't
    /// cross-origin isolated.
    #[wasm_bindgen(getter)]
    pub fn sab(&self) -> Option<SharedArrayBuffer> {
        self.engine.sab.clone()
    }

    /// Rects put on the canvas since mount, for diagnostics.
    pub fn puts(&self) -> u32 {
        self.host.puts()
    }

    /// Views made of the frame since mount, for diagnostics.
    pub fn binds(&self) -> u32 {
        self.host.binds()
    }
}

/// Mount the gain editor on `canvas` and start drawing.
#[wasm_bindgen]
pub fn start(canvas: HtmlCanvasElement) -> Result<GainPage, JsValue> {
    let initial: Vec<f32> = gain::PARAMS.iter().map(|p| p.default).collect();
    let (link, engine) = connect(&initial)?;
    let host = silkscreen_host_web::mount(canvas, gain::editor(link))?;
    Ok(GainPage { host, engine })
}
