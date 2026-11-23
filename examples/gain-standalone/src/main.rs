//! The gain example as a desktop app: the editor in a top-level
//! window and the engine on the default audio device, joined by the native
//! bridge.
//!
//! ```sh
//! cargo run -p gain-standalone                # a test tone through the gain
//! cargo run -p gain-standalone -- --input mic # the input device (headphones!)
//! cargo run -p gain-standalone -- --input none # silence
//! ```
//!
//! Drag the knob (Shift for fine), double-click or Cmd-click to reset,
//! scroll to step. Resizing snaps to the zoom steps; keys 1–5 pick one.
//! Edit ▸ Undo (⌘Z) and Redo (⇧⌘Z) step through knob moves. Files dropped
//! on the knob are printed.
//!

mod drop;

use gain::engine::GainEngine;
use silkscreen::bridge::bridge;
use silkscreen::core::EmbeddedRaster;
use silkscreen::{Binding, Editor, EditorState, EngineLink, ParamKnob};
use silkscreen_cpal::Source;
use silkscreen_host_native::{WindowOptions, run_standalone};

fn main() {
    let source = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => Source::TestTone,
        [flag, input] if flag == "--input" && input == "mic" => Source::Input,
        [flag, input] if flag == "--input" && input == "none" => Source::Silence,
        _ => {
            eprintln!("usage: gain-standalone [--input mic|none]");
            std::process::exit(2);
        }
    };
    let initial: Vec<f32> = gain::PARAMS.iter().map(|p| p.default).collect();
    let (ui, end) = bridge(&initial, silkscreen::bridge::RING_CAPACITY);
    // Held until the window closes; dropping it stops the audio.
    let audio = match silkscreen_cpal::start(GainEngine::new, end, source) {
        Ok(audio) => {
            println!("gain-standalone: audio at {} Hz", audio.sample_rate());
            Some(audio)
        }
        Err(e) => {
            eprintln!("gain-standalone: no audio ({e}); the editor runs without it");
            None
        }
    };
    let options = WindowOptions::new("silkscreen: gain", gain::faceplate::SIZE);
    let result = run_standalone(options, move || editor(Box::new(ui)));
    if audio.as_ref().is_some_and(silkscreen_cpal::Audio::failed) {
        eprintln!("gain-standalone: the engine panicked; audio went silent");
    }
    drop(audio);
    if let Err(e) = result {
        eprintln!("gain-standalone: {e}");
        std::process::exit(1);
    }
}

/// Gain's editor, with the knob reporting file drops.
fn editor(link: Box<dyn EngineLink>) -> Editor {
    let controls = gain::faceplate::Controls::<Box<dyn Binding>> {
        gain: Box::new(drop::DropLogger(ParamKnob::new(gain::GAIN))),
    };
    let state = EditorState::new(
        &gain::faceplate::LAYOUT,
        &gain::PARAMS,
        controls.into_array(),
        link,
    );
    Editor::new(
        state,
        Box::new(EmbeddedRaster::new(&gain::faceplate::FACEPLATE)),
    )
}
