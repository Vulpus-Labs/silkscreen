//! The tone example as a CLAP instrument: proof that note input reaches an
//! engine. The params, editor and engine are the `tone` crate's.

use std::ffi::CStr;

use silkscreen::PluginUi;
use silkscreen_clap::{SilkscreenPlugin, export_clap, features};
pub use tone::engine::ToneEngine;
pub use tone::{LEVEL, OCTAVE, PARAMS, WAVE};

#[derive(Debug)]
pub struct TonePlugin;

impl SilkscreenPlugin for TonePlugin {
    const ID: &'static str = "com.example.silkscreen.tone";
    const NAME: &'static str = "silkscreen Tone";
    const VERSION: &'static str = tone::UI.version;
    const FEATURES: &'static [&'static CStr] = &[
        features::INSTRUMENT,
        features::SYNTHESIZER,
        features::STEREO,
    ];
    const BUILD: &'static str = env!("SILKSCREEN_BUILD_ID");
    /// The badge shows the transport, for checking it in a DAW.
    const DIAGNOSTICS: bool = true;
    const AUDIO_INPUT: bool = false;
    const NOTE_INPUT: bool = true;
    const UI: PluginUi = tone::UI;

    type Engine = ToneEngine;

    fn engine(sample_rate: f32, _max_frames: u32) -> ToneEngine {
        ToneEngine::new(sample_rate)
    }
}

export_clap!(TonePlugin);
