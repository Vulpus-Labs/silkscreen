//! The selector: widget, `ParamSelector` binding, snapshots.

use silkscreen::EngineEvent;
use silkscreen::core::{FaderGeometry, Point};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{SELECT, WAVE, faceplate, faceplate::ids};

fn geometry(id: silkscreen::ControlId) -> FaderGeometry {
    FaderGeometry::new(faceplate::LAYOUT.control(id).rect)
}

/// Centre of position `i` of `n` on selector `id`.
fn position(id: silkscreen::ControlId, i: u16, n: u16) -> Point {
    geometry(id).cap(f32::from(i) / f32::from(n - 1)).center()
}

#[test]
fn clicking_selects_the_nearest_position() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let p = position(ids::WAVE, 3, 5);
    h.click_at(Point::new(p.x, p.y + 4.0));
    assert_eq!(h.model().get(WAVE), 3.0);
    assert_eq!(
        h.engine_events(),
        [
            EngineEvent::GestureBegin { index: WAVE },
            EngineEvent::ParamValue {
                index: WAVE,
                value: 3.0
            },
            EngineEvent::GestureEnd { index: WAVE },
        ]
    );
}

#[test]
fn dragging_snaps_and_only_whole_positions_reach_the_model() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let g = geometry(ids::SELECT);
    let start = position(ids::SELECT, 0, 6);
    // Up by 2.4 positions' worth: lands on 2.
    h.drag_from(start, -g.travel() * 2.4 / 5.0);
    assert_eq!(h.model().get(SELECT), 2.0);
    for e in h.engine_events() {
        if let EngineEvent::ParamValue { value, .. } = e {
            assert_eq!(value, value.round(), "{value}");
        }
    }
}

#[test]
fn wheel_steps_one_position() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.wheel(ids::WAVE, 1.0).wheel(ids::WAVE, 1.0);
    assert_eq!(h.model().get(WAVE), 2.0);
    h.wheel(ids::WAVE, -1.0);
    assert_eq!(h.model().get(WAVE), 1.0);
}

#[test]
fn snapshots_of_every_position() {
    for (id, n, name) in [(ids::WAVE, 5, "wave"), (ids::SELECT, 6, "select")] {
        let mut h = Harness::new(widgets::editor, 2.0);
        for i in 0..n {
            h.click_at(position(id, i, n));
            h.advance(1000.0);
            h.move_to(Point::new(1.0, 1.0));
            assert_snapshot!(h, &format!("selector_{name}_{i}@2x"));
        }
    }
}
