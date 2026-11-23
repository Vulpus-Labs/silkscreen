//! Controls dimmed by a predicate over the model.

use silkscreen::core::Point;
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, LEVEL, SYNC, faceplate, faceplate::ids};

fn away(h: &mut Harness) {
    h.advance(1000.0);
    h.move_to(Point::new(1.0, 1.0));
}

#[test]
fn a_dim_input_changing_repaints_the_dimmed_controls() {
    let mut h = Harness::new(widgets::dimmed_editor, 1.0);
    away(&mut h);
    h.snapshot();
    h.click(ids::SYNC);
    away(&mut h);
    h.snapshot();
    let damage = h.last_damage().to_vec();
    for id in [ids::LEVEL, ids::FINE] {
        let r = faceplate::LAYOUT.control(id).rect.to_physical(1.0);
        assert!(
            damage.iter().any(|d| d.intersect(&r).is_some()),
            "{id:?} not repainted: {damage:?}"
        );
    }
    let cutoff = faceplate::LAYOUT.control(ids::CUTOFF).rect.to_physical(1.0);
    assert!(
        !damage.iter().any(|d| d.intersect(&cutoff).is_some()),
        "Cutoff doesn't read Sync: {damage:?}"
    );
    assert_eq!(h.model().get(SYNC), 1.0);
}

#[test]
fn a_dimmed_control_still_works() {
    let mut h = Harness::new(widgets::dimmed_editor, 1.0);
    h.click(ids::SYNC);
    away(&mut h);
    let level = h.model().get(LEVEL);
    h.drag(ids::LEVEL, -20.0);
    assert_ne!(h.model().get(LEVEL), level, "the dimmed knob turned");
    away(&mut h);
    let cutoff = h.model().get(CUTOFF);
    h.drag(ids::CUTOFF, -20.0);
    assert_ne!(h.model().get(CUTOFF), cutoff, "the undimmed fader moved");
}

#[test]
fn snapshots_of_dimmed_controls() {
    for scale in [1.0, 2.0] {
        let mut h = Harness::new(widgets::dimmed_editor, scale);
        away(&mut h);
        h.click(ids::SYNC);
        away(&mut h);
        h.click(ids::STOP);
        away(&mut h);
        assert_snapshot!(h, &format!("dimmed@{scale}x"));
    }
}
