//! The fader: widget, `ParamFader` binding, snapshots.

use silkscreen::EngineEvent;
use silkscreen::core::{FaderGeometry, Point};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, faceplate, faceplate::ids};

fn geometry() -> FaderGeometry {
    FaderGeometry::new(faceplate::LAYOUT.control(ids::CUTOFF).rect)
}

fn cap_centre(h: &Harness) -> Point {
    geometry().cap(h.model().get_normalized(CUTOFF)).center()
}

#[test]
fn dragging_the_cap_follows_the_pointer() {
    let mut h = Harness::new(widgets::editor, 1.0);
    assert_eq!(h.model().get(CUTOFF), 0.5);
    let travel = geometry().travel();
    let start = cap_centre(&h);
    h.drag_from(start, -travel * 0.25);
    assert!(
        (h.model().get(CUTOFF) - 0.75).abs() < 1e-4,
        "{}",
        h.model().get(CUTOFF)
    );
    let events = h.engine_events();
    assert_eq!(
        events.first(),
        Some(&EngineEvent::GestureBegin { index: CUTOFF })
    );
    assert_eq!(
        events.last(),
        Some(&EngineEvent::GestureEnd { index: CUTOFF })
    );
}

#[test]
fn clicking_the_track_jumps_there_as_one_gesture() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let g = geometry();
    let target = g.cap(0.2).center();
    h.click_at(target);
    assert!(
        (h.model().get(CUTOFF) - 0.2).abs() < 1e-4,
        "{}",
        h.model().get(CUTOFF)
    );
    let events = h.engine_events();
    assert_eq!(events.len(), 3, "{events:?}");

    // Clicking the cap itself changes nothing. (Wait first: the cap is now
    // under the pointer, so an immediate second click is a double-click.)
    h.advance(1000.0);
    h.click_at(cap_centre(&h));
    assert!(h.engine_events().is_empty());
}

#[test]
fn double_click_resets_and_wheel_steps() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.click_at(geometry().cap(0.9).center());
    h.advance(1000.0);
    let at = cap_centre(&h);
    h.click_at(at).click_at(at);
    assert_eq!(h.model().get(CUTOFF), 0.5);
    h.wheel(ids::CUTOFF, 3.0);
    assert!((h.model().get(CUTOFF) - 0.53).abs() < 1e-4);
}

#[test]
fn snapshots() {
    for scale in [1.0, 2.0] {
        let mut h = Harness::new(widgets::editor, scale);
        h.click_at(geometry().cap(0.8).center());
        h.move_to(Point::new(1.0, 1.0));
        assert_snapshot!(h, &format!("fader_0.8@{scale}x"));
        // A change repaints only the fader.
        let fader = faceplate::LAYOUT
            .control(ids::CUTOFF)
            .rect
            .to_physical(scale);
        h.click_at(geometry().cap(0.1).center());
        h.move_to(Point::new(1.0, 1.0));
        h.snapshot();
        assert!(
            h.last_damage()
                .iter()
                .all(|d| d.intersect(&fader) == Some(*d)),
            "{:?}",
            h.last_damage()
        );
    }
}
