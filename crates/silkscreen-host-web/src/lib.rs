//! Web host for silkscreen editors ([design/hosts.md]): the editor draws into a
//! frame in wasm memory, presented to a `<canvas>` with `putImageData` over
//! the damage rects, once per `requestAnimationFrame`.
//!
//! [`mount`] takes an [`Editor`](silkscreen::Editor) and a canvas, sizes
//! the canvas's backing store to CSS size × `devicePixelRatio`, and follows
//! the ratio (browser zoom, moving between displays) from frame to frame.
//!
//! Faceplates built in fetch mode ([design/hosts.md]) name their PNGs by URL.
//! The loop fetches each one when a raster first needs it, relative to the
//! page, and repaints in full when it arrives; the best PNG at hand stands
//! in meanwhile.
//!
//! Input arrives through DOM listeners on the canvas ([design/hosts.md]). A
//! drag captures the pointer, so it carries on outside the canvas and the
//! window. Dropped files reach the editor as handles in
//! [`DropPayload::Files`](silkscreen::core::DropPayload::Files); resolve
//! them with [`take_dropped_file`].
//!
//! The presentation logic (`Presenter`) and the event translation
//! (`translate`) are target-independent, so they are unit-tested
//! natively; only `CanvasSurface`, the loop and the listeners touch the
//! DOM.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

mod export_page;
mod files;
mod host;
#[cfg(target_arch = "wasm32")]
mod idb;
mod input;
mod memory;
mod page;
mod present;
mod preset_page;
// Parts of it only the wasm32 session reaches; natively, only tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod restore;
#[cfg(target_arch = "wasm32")]
mod session;
// Parts of it only the wasm32 session reaches; natively, only tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod share;
// Parts of it only the wasm32 session reaches; natively, only tests.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod store;
mod surface;
mod translate;
mod transport;

pub use files::take_dropped_file;
pub use host::{WebHost, mount};
pub use page::{Connection, connect, cross_origin_isolated};
pub use preset_page::PresetPage;

/// This crate's JavaScript (the worklet, page, keyboard, MIDI and survey
/// modules), on the machine that built it: what a web page serves beside
/// its wasm, and what a page's node tests import.
pub const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/js");

/// What [`export_page!`] expands to, so a synth's crate needn't depend on
/// it. Not part of the API.
#[doc(hidden)]
pub mod __private {
    pub use js_sys;
    pub use wasm_bindgen;
    pub use wasm_bindgen_futures;
    pub use web_sys;
}

#[cfg(target_arch = "wasm32")]
use idb::open_preset_store;

/// Off the web (native builds and tests of pages), presets last in memory.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::unused_async)]
async fn open_preset_store(_plugin: &str) -> store::JournaledStore {
    store::JournaledStore::default()
}
