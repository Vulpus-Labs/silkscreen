//! The editor, driven headlessly: scripted input in, params and pixels out.
//! Snapshots live in `tests/snapshots/`; after a deliberate visual change,
//! re-bless them with `SILKSCREEN_BLESS=1 cargo test` and look at the diffs.

use my_synth::{LEVEL, faceplate::ids};
use silkscreen_harness::{Harness, assert_snapshot};

#[test]
fn the_faceplate_draws() {
    let mut h = Harness::new(my_synth::editor, 1.0);
    assert_snapshot!(h, "initial_1x");
}

#[test]
fn dragging_the_level_knob_raises_the_level() {
    let mut h = Harness::new(my_synth::editor, 1.0);
    let before = h.model().get(LEVEL);
    h.drag(ids::LEVEL, -20.0);
    assert!(h.model().get(LEVEL) > before);
}
