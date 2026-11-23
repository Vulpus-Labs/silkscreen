//! A mono instrument: proof that note input reaches an engine. It reuses
//! the gain example's faceplate (its knob is the output level) and adds two
//! discrete params with no control, for the host's generic UI: a stepped
//! waveform and an octave toggle.
//!
//! The params, the editor and the engine, shared by `tone-plugin` (CLAP)
//! and the web crates, as gain's are.

use silkscreen::{
    Binding, Editor, EditorState, EngineLink, ParamIndex, ParamKnob, ParamSpec, PluginUi,
};

pub mod engine;
pub mod held;

pub const LEVEL: ParamIndex = gain::GAIN;
pub const WAVE: ParamIndex = ParamIndex(1);
pub const OCTAVE: ParamIndex = ParamIndex(2);

pub static PARAMS: [ParamSpec; 3] = [
    gain::PARAMS[0],
    ParamSpec::stepped("Wave", &["Sine", "Triangle", "Square"], 0).id(2),
    ParamSpec::toggle("Octave", false)
        .labels(&["Normal", "Down"])
        .id(3),
];

/// The long-lived editor state, bound to `link`.
pub fn state(link: Box<dyn EngineLink>) -> EditorState {
    let controls = gain::faceplate::Controls::<Box<dyn Binding>> {
        gain: Box::new(ParamKnob::new(LEVEL)),
    };
    EditorState::new(
        &gain::faceplate::LAYOUT,
        &PARAMS,
        controls.into_array(),
        link,
    )
}

/// The tone example's editor side, for its CLAP plugin.
pub const UI: PluginUi = PluginUi {
    vendor: "Example Audio",
    product: "Tone",
    version: env!("CARGO_PKG_VERSION"),
    web_prefix: "tone",
    specs: &PARAMS,
    factory: &[],
    faceplate: &gain::faceplate::FACEPLATE,
    state,
};

/// A complete editor (state + view), for hosts that own both.
pub fn editor(link: Box<dyn EngineLink>) -> Editor {
    Editor::new(state(link), UI.raster())
}
