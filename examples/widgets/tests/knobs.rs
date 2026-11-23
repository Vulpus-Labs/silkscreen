//! The sprite knob (base + rotating top) and the bipolar knob.

use silkscreen::core::Point;
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{FINE, LEVEL, faceplate::ids};

fn away(h: &mut Harness) {
    h.advance(1000.0);
    h.move_to(Point::new(1.0, 1.0));
}

#[test]
fn the_bipolar_knob_rests_at_its_centre() {
    let h = Harness::new(widgets::editor, 1.0);
    assert_eq!(h.model().get(FINE), 0.0);
    assert_eq!(h.model().get_normalized(FINE), 0.5);
}

#[test]
fn snapshots() {
    let mut h = Harness::new(widgets::editor, 2.0);
    // LEVEL is the sprite knob; FINE the bipolar vector knob. Dragging up
    // increases; 200 px is the full range.
    for (name, level, fine) in [("max", -200.0, -50.0), ("min", 200.0, 100.0)] {
        h.drag(ids::LEVEL, level);
        away(&mut h);
        h.drag(ids::FINE, fine);
        away(&mut h);
        assert_snapshot!(h, &format!("knobs_{name}@2x"));
    }
    assert_eq!(h.model().get(LEVEL), 0.0);
    assert!(
        (h.model().get(FINE) - -25.0).abs() < 0.01,
        "{}",
        h.model().get(FINE)
    );
}

#[test]
fn bipolar_default_snapshot() {
    let mut h = Harness::new(widgets::editor, 2.0);
    away(&mut h);
    assert_snapshot!(h, "knobs_default@2x");
}
