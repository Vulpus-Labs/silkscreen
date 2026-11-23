//! The gain example as a CLAP plugin: `gain`'s editor and engine behind
//! `silkscreen-clap`.

use std::ffi::CStr;

use gain::engine::GainEngine;
use silkscreen::PluginUi;
use silkscreen_clap::{SilkscreenPlugin, export_clap, features};

#[derive(Debug)]
pub struct GainPlugin;

impl SilkscreenPlugin for GainPlugin {
    const ID: &'static str = "com.example.silkscreen.gain";
    const NAME: &'static str = "silkscreen Gain";
    const VERSION: &'static str = gain::UI.version;
    const FEATURES: &'static [&'static CStr] = &[features::AUDIO_EFFECT, features::STEREO];
    const BUILD: &'static str = env!("SILKSCREEN_BUILD_ID");
    const DIAGNOSTICS: bool = false;
    const AUDIO_INPUT: bool = true;
    const UI: PluginUi = gain::UI;

    type Engine = GainEngine;

    fn engine(sample_rate: f32, _max_frames: u32) -> GainEngine {
        GainEngine::new(sample_rate)
    }
}

export_clap!(GainPlugin);
