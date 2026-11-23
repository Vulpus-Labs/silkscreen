//! The end-to-end proof: SVG → generated layout → editor → scripted
//! input → model, engine events and pixels.

use gain::{GAIN, faceplate::ids};
use silkscreen::EngineEvent;
use silkscreen::core::tiny_skia::Pixmap;
use silkscreen_harness::{Harness, assert_snapshot};

#[test]
fn initial_frame_1x() {
    let mut h = Harness::new(gain::editor, 1.0);
    assert_snapshot!(h, "initial_1x");
}

#[test]
fn initial_frame_2x() {
    let mut h = Harness::new(gain::editor, 2.0);
    assert_snapshot!(h, "initial_2x");
}

#[test]
fn after_drag_1x() {
    let mut h = Harness::new(gain::editor, 1.0);
    h.drag(ids::GAIN, -20.0);
    assert_snapshot!(h, "after_drag_1x");
}

#[test]
fn after_drag_2x() {
    let mut h = Harness::new(gain::editor, 2.0);
    h.drag(ids::GAIN, -20.0);
    assert_snapshot!(h, "after_drag_2x");
}

#[test]
fn drag_replays_as_one_bracketed_gesture() {
    let mut h = Harness::new(gain::editor, 1.0);
    h.drag(ids::GAIN, -20.0);

    // 200 px is the full 72 dB range, so 20 px up from 0 dB is +7.2 dB.
    let gain = h.model().get(GAIN);
    assert!((gain - 7.2).abs() < 1e-3, "{gain}");

    let events = h.engine_events();
    let shape: String = events
        .iter()
        .map(|e| match e {
            EngineEvent::GestureBegin { index } if *index == GAIN => 'B',
            EngineEvent::ParamValue { index, .. } if *index == GAIN => 'V',
            EngineEvent::GestureEnd { index } if *index == GAIN => 'E',
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    // One value per pointer move.
    assert_eq!(
        shape,
        format!("B{}E", "V".repeat(Harness::DRAG_STEPS as usize))
    );
    assert_eq!(
        events[events.len() - 2],
        EngineEvent::ParamValue {
            index: GAIN,
            value: gain
        }
    );
}

#[test]
fn drag_changes_pixels_only_inside_the_knob() {
    for scale in [1.0, 1.5, 2.0] {
        let mut h = Harness::new(gain::editor, scale);
        let before: Pixmap = h.snapshot().clone();
        h.drag(ids::GAIN, -20.0);
        let after = h.snapshot();

        let knob = gain::faceplate::LAYOUT
            .control(ids::GAIN)
            .rect
            .to_physical(scale);
        let w = before.width();
        let mut changed = 0;
        for (i, (a, b)) in before.pixels().iter().zip(after.pixels()).enumerate() {
            if a != b {
                changed += 1;
                let (x, y) = ((i as u32 % w) as i32, (i as u32 / w) as i32);
                assert!(
                    x >= knob.x && x < knob.right() && y >= knob.y && y < knob.bottom(),
                    "pixel ({x}, {y}) changed outside the knob at {scale}x"
                );
            }
        }
        assert!(changed > 0, "the drag changed nothing at {scale}x");
    }
}

#[test]
fn reset_returns_to_default() {
    let mut h = Harness::new(gain::editor, 1.0);
    h.drag(ids::GAIN, 60.0).advance(1000.0);
    assert!(h.model().get(GAIN) < -20.0);
    h.double_click(ids::GAIN);
    assert_eq!(h.model().get(GAIN), 0.0);
}
