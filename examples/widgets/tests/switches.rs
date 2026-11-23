//! Toggle, latching button and radio group: sprites, `ParamToggle`,
//! `ParamRadio`.

use silkscreen::EngineEvent;
use silkscreen::core::Point;
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{STOP, SYNC, TUNING, faceplate, faceplate::ids};

fn away(h: &mut Harness) {
    h.advance(1000.0);
    h.move_to(Point::new(1.0, 1.0));
}

#[test]
fn a_toggle_flips_on_each_click_as_a_gesture() {
    let mut h = Harness::new(widgets::editor, 1.0);
    assert_eq!(h.model().get(STOP), 0.0);
    h.click(ids::STOP);
    assert_eq!(h.model().get(STOP), 1.0);
    assert_eq!(
        h.engine_events(),
        [
            EngineEvent::GestureBegin { index: STOP },
            EngineEvent::ParamValue {
                index: STOP,
                value: 1.0
            },
            EngineEvent::GestureEnd { index: STOP },
        ]
    );
    away(&mut h);
    h.click(ids::STOP);
    assert_eq!(h.model().get(STOP), 0.0);
}

#[test]
fn the_button_latches() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.click(ids::SYNC);
    assert_eq!(h.model().get(SYNC), 1.0, "stays on after release");
    away(&mut h);
    h.click(ids::SYNC);
    assert_eq!(h.model().get(SYNC), 0.0);
}

#[test]
fn radio_buttons_select_their_position_and_all_redraw() {
    let mut h = Harness::new(widgets::editor, 1.0);
    assert_eq!(h.model().get(TUNING), 2.0);
    away(&mut h);
    h.snapshot();
    h.click(ids::TUNING_0);
    assert_eq!(h.model().get(TUNING), 0.0);
    away(&mut h);
    h.snapshot();
    let damage = h.last_damage().to_vec();
    for id in faceplate::radios::TUNING {
        let r = faceplate::LAYOUT.control(id).rect.to_physical(1.0);
        assert!(
            damage.iter().any(|d| d.intersect(&r).is_some()),
            "{id:?} not repainted: {damage:?}"
        );
    }
    // Clicking the selected button again changes nothing.
    let _ = h.engine_events();
    h.click(ids::TUNING_0);
    assert!(h.engine_events().is_empty());
}

#[test]
fn snapshots_of_both_frames() {
    for scale in [1.0, 2.0] {
        let mut h = Harness::new(widgets::editor, scale);
        away(&mut h);
        assert_snapshot!(h, &format!("switches_off@{scale}x"));
        h.click(ids::STOP);
        away(&mut h);
        h.click(ids::SYNC);
        away(&mut h);
        h.click(ids::TUNING_1);
        away(&mut h);
        assert_snapshot!(h, &format!("switches_on@{scale}x"));
    }
}
