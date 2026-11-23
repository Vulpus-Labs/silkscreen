//! My Synth in a window of its own: `cargo run -p my-synth-standalone`.
//! The engine runs on the default audio device. The standalone host takes
//! no notes yet, so the app is for the faceplate and its params; play the
//! synth in a DAW (my-synth-clap) or on its web page (my-synth-web).

use my_synth::engine::MyEngine;
use silkscreen::bridge::bridge;
use silkscreen_cpal::Source;
use silkscreen_host_native::{WindowOptions, run_standalone};

fn main() {
    let initial: Vec<f32> = my_synth::PARAMS.iter().map(|p| p.default).collect();
    let (ui, end) = bridge(&initial, silkscreen::bridge::RING_CAPACITY);
    // Held until the window closes: dropping it stops the audio.
    let audio = match silkscreen_cpal::start(MyEngine::new, end, Source::Silence) {
        Ok(audio) => Some(audio),
        Err(e) => {
            eprintln!("my-synth: no audio ({e}); the editor runs without it");
            None
        }
    };
    let options = WindowOptions::new("My Synth", my_synth::faceplate::SIZE);
    let result = run_standalone(options, move || my_synth::editor(Box::new(ui)));
    drop(audio);
    if let Err(e) = result {
        eprintln!("my-synth: {e}");
        std::process::exit(1);
    }
}
