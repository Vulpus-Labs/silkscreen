//! Opens the widgets faceplate in a native window, with no audio, for
//! trying every control kind by hand: `cargo run -p widgets --example preview`.
//!
//! - LEVEL (knob) and FINE (bipolar): drag, Shift for fine,
//!   double-click to reset, scroll to step.
//! - CUTOFF (fader): drag the cap, click the track to jump.
//! - WAVE and SELECT (selectors): drag between detents, click a position,
//!   scroll to step.
//! - STOP (toggle) and SYNC (button): click to flip.
//! - TUNING (radio): click a button to select it.
//! - MODE (drop-down): click to choose; scroll to step.
//! - Program (preset field): click for the preset browser; presets are
//!   saved in `~/Library/Application Support/silkscreen widgets/Presets`.
//!
//! Keys 1–5 zoom 75/100/125/150/200%.

use silkscreen::bridge::NullLink;
use silkscreen_host_native::{DirStore, WindowOptions, run_standalone};

fn main() {
    let options = WindowOptions::new("silkscreen: widgets", widgets::faceplate::SIZE);
    let store =
        DirStore::default_for("Example Audio", "silkscreen widgets").expect("a home folder");
    eprintln!("presets in {}", store.dir().display());
    if let Err(e) = run_standalone(options, move || {
        widgets::editor_with(Box::new(NullLink), Box::new(store))
    }) {
        eprintln!("widgets preview: {e}");
        std::process::exit(1);
    }
}
