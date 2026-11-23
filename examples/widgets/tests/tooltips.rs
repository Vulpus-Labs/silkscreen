//! Tooltips and the drag readout (design/overlays-and-text.md).

use silkscreen::core::{Modifiers, MouseButton, Point, RawKind, Vec2};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::faceplate::ids;

#[test]
fn resting_on_a_control_shows_its_name_and_value() {
    let mut h = Harness::new(widgets::editor, 2.0);
    let c = h.centre(ids::CUTOFF);
    h.move_to(c).advance(100.0);
    assert_eq!(h.editor().tip(), None, "not before the dwell");
    h.advance(600.0);
    assert_eq!(h.editor().tip(), Some("Cutoff  0.50"));
    assert_snapshot!(h, "tooltip_cutoff@2x");

    // Moving hides it; leaving the control keeps it hidden.
    h.move_to(c + Vec2::new(1.0, 0.0));
    assert_eq!(h.editor().tip(), None);
    h.advance(600.0);
    assert_eq!(h.editor().tip(), Some("Cutoff  0.50"), "rests again");
    h.move_to(Point::new(1.0, 1.0)).advance(600.0);
    assert_eq!(h.editor().tip(), None);
}

#[test]
fn a_drag_reads_out_the_value_as_it_changes() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let start = h.centre(ids::WAVE);
    h.move_to(start).raw(RawKind::PointerDown {
        pos: start,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    h.advance(16.0);
    assert_eq!(h.editor().tip(), None, "a press alone shows nothing");
    let up = start + Vec2::new(0.0, -80.0);
    h.raw(RawKind::PointerMove {
        pos: up,
        mods: Modifiers::NONE,
    })
    .advance(16.0);
    let tip = h.editor().tip().expect("dragging reads out").to_owned();
    assert!(tip.starts_with("Wave  ") && tip != "Wave  Saw", "{tip}");
    h.raw(RawKind::PointerUp {
        pos: up,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    })
    .advance(16.0);
    assert_eq!(h.editor().tip(), None, "gone when the drag ends");
}

#[test]
fn a_tip_near_the_foot_stays_inside_the_window() {
    // TUNING_2 sits near the faceplate's foot; the tip just fits below
    // (placing it above when it doesn't is unit-tested in `tooltip`).
    let mut h = Harness::new(widgets::editor, 2.0);
    let c = h.centre(ids::TUNING_2);
    h.move_to(c).advance(700.0);
    assert_eq!(h.editor().tip(), Some("Tuning  Equal"));
    assert_snapshot!(h, "tooltip_tuning@2x");
}

#[test]
fn a_plugin_theme_colours_the_tooltip() {
    let blue = [0, 0, 255, 255];
    let mut h = Harness::new(
        |link| {
            let mut editor = widgets::editor(link);
            editor.state.set_theme(silkscreen::Theme {
                background: blue,
                ..silkscreen::Theme::default()
            });
            editor
        },
        1.0,
    );
    let c = h.centre(ids::CUTOFF);
    h.move_to(c).advance(700.0);
    assert!(h.editor().tip().is_some());
    let frame = h.snapshot();
    let blues = frame
        .pixels()
        .iter()
        .filter(|p| (p.red(), p.green(), p.blue()) == (0, 0, 255))
        .count();
    assert!(
        blues > 100,
        "the tip's background is the theme's: {blues} px"
    );
}
