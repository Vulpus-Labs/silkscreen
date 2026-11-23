//! My Synth as a CLAP instrument: the editor and engine of `my-synth`
//! behind `silkscreen-clap`. `cargo silkscreen bundle my-synth-clap
//! --install` builds the `.clap` and puts it where hosts look.

use std::ffi::CStr;

use my_synth::engine::MyEngine;
use silkscreen::PluginUi;
use silkscreen_clap::{SilkscreenPlugin, export_clap, features};

#[derive(Debug)]
pub struct MySynthPlugin;

impl SilkscreenPlugin for MySynthPlugin {
    /// Reverse-DNS, and never changed once released: hosts save projects
    /// under it.
    const ID: &'static str = "com.example.my-synth";
    const NAME: &'static str = "My Synth";
    const VERSION: &'static str = my_synth::UI.version;
    const FEATURES: &'static [&'static CStr] = &[
        features::INSTRUMENT,
        features::SYNTHESIZER,
        features::STEREO,
    ];
    const AUDIO_INPUT: bool = false;
    const NOTE_INPUT: bool = true;
    const USER_PRESETS: bool = true;
    const UI: PluginUi = my_synth::UI;

    type Engine = MyEngine;

    fn engine(sample_rate: f32, _max_frames: u32) -> MyEngine {
        MyEngine::new(sample_rate)
    }
}

export_clap!(MySynthPlugin);
