//! silkscreen example: one SVG faceplate, one gain knob.
//!
//! `gain.svg` is the whole UI design. `build.rs` turns it into embedded
//! PNGs and the `faceplate` module below; this file only declares the
//! params and binds each control.

use silkscreen::{
    Binding, Editor, EditorState, EngineLink, ParamIndex, ParamKnob, ParamSpec, PluginUi,
};

pub mod engine;
pub mod smooth;

/// Generated from `gain.svg` by `silkscreen-build`.
pub mod faceplate {
    include!(concat!(env!("OUT_DIR"), "/gain.rs"));
}

pub const GAIN: ParamIndex = ParamIndex(0);

pub static PARAMS: [ParamSpec; 1] = [ParamSpec::new("Gain", -60.0, 12.0, 0.0)
    .unit("dB")
    .decimals(1)
    .id(1)];

/// The long-lived editor state, bound to `link`.
pub fn state(link: Box<dyn EngineLink>) -> EditorState {
    let controls = faceplate::Controls::<Box<dyn Binding>> {
        gain: Box::new(ParamKnob::new(GAIN)),
    };
    EditorState::new(&faceplate::LAYOUT, &PARAMS, controls.into_array(), link)
}

/// The gain example's editor side, for its CLAP plugin.
pub const UI: PluginUi = PluginUi {
    vendor: "Example Audio",
    product: "Gain",
    version: env!("CARGO_PKG_VERSION"),
    web_prefix: "gain",
    specs: &PARAMS,
    factory: &[],
    faceplate: &faceplate::FACEPLATE,
    frame: 0,
    state,
};

/// A complete editor (state + view), for hosts that own both.
pub fn editor(link: Box<dyn EngineLink>) -> Editor {
    Editor::new(state(link), UI.raster())
}
