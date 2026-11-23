//! Opens the gain example's editor in a native window, with no audio.
//!
//! Drag the knob (Shift for fine), double-click or Cmd-click to reset,
//! scroll to step. Keys 1–5 zoom 75/100/125/150/200%.

use silkscreen::bridge::NullLink;
use silkscreen_host_native::{WindowOptions, run_standalone};

fn main() {
    let options = WindowOptions::new("silkscreen: gain", gain::faceplate::SIZE);
    if let Err(e) = run_standalone(options, || gain::editor(Box::new(NullLink))) {
        eprintln!("gain-preview: {e}");
        std::process::exit(1);
    }
}
