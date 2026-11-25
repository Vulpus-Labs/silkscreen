//! My Synth: its parameters, its editor and its engine, shared by the CLAP
//! plugin, the web page and the desktop app.
//!
//! `my-synth.svg` is the whole visual design. `build.rs` turns it into
//! PNGs and the `faceplate` module below; this file declares the params
//! and binds each control to one.

use silkscreen::{
    Binding, Editor, EditorState, EngineLink, ParamIndex, ParamKnob, ParamSpec, PluginUi,
};

pub mod engine;

/// Generated from `my-synth.svg` by `silkscreen-build`.
pub mod faceplate {
    include!(concat!(env!("OUT_DIR"), "/my-synth.rs"));
}

pub const WAVE: ParamIndex = ParamIndex(0);
pub const LEVEL: ParamIndex = ParamIndex(1);

/// The parameter table: index `i` is `ParamIndex(i)`. Each `id` is what
/// presets and host projects store, so never change or reuse one.
pub static PARAMS: [ParamSpec; 2] = [
    ParamSpec::stepped("Wave", &["Sine", "Triangle", "Square"], 0).id(1),
    ParamSpec::new("Level", -48.0, 0.0, -6.0)
        .unit("dB")
        .decimals(1)
        .id(2),
];

/// The editor's long-lived state, bound to the engine through `link`.
pub fn state(link: Box<dyn EngineLink>) -> EditorState {
    let controls = faceplate::Controls::<Box<dyn Binding>> {
        wave: Box::new(ParamKnob::new(WAVE)),
        level: Box::new(ParamKnob::new(LEVEL)),
    };
    EditorState::new(&faceplate::LAYOUT, &PARAMS, controls.into_array(), link)
}

/// What every host needs to know about the editor.
pub const UI: PluginUi = PluginUi {
    vendor: "My Company",
    product: "My Synth",
    version: env!("CARGO_PKG_VERSION"),
    web_prefix: "my-synth",
    specs: &PARAMS,
    factory: &[],
    faceplate: &faceplate::FACEPLATE,
    frame: 0,
    state,
};

/// A whole editor, state and view, for hosts that own both.
pub fn editor(link: Box<dyn EngineLink>) -> Editor {
    Editor::new(state(link), UI.raster())
}
