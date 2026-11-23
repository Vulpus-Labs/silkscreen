//! The drop-down: a field over a stepped param that opens a list.

use silkscreen::EngineEvent;
use silkscreen::core::{Key, Modifiers, Point, RawKind};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{MODE, faceplate::ids};

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

#[test]
fn clicking_opens_the_choices_and_choosing_commits_one_gesture() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.click(ids::MODE);
    assert_eq!(h.editor().panels(), 1, "the list opened");
    let _ = h.engine_events();
    // The list opens below the field with "Mono" selected; Down twice and
    // Enter chooses "Unison".
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 0, "closed on choice");
    assert_eq!(h.model().get(MODE), 2.0);
    assert_eq!(
        h.engine_events(),
        [
            EngineEvent::GestureBegin { index: MODE },
            EngineEvent::ParamValue {
                index: MODE,
                value: 2.0
            },
            EngineEvent::GestureEnd { index: MODE },
        ]
    );
}

#[test]
fn a_click_on_a_row_chooses_it_and_escape_cancels() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let field = widgets::faceplate::LAYOUT.control(ids::MODE).rect;
    h.click(ids::MODE);
    // The fourth row, "Legato": 2 below the field, 3 into the list.
    let y = field.bottom() + 2.0 + 3.0 + 3.5 * silkscreen::list::ROW_H;
    h.click_at(Point::new(field.x + 20.0, y));
    assert_eq!(h.model().get(MODE), 3.0);

    h.advance(500.0).click(ids::MODE);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(h.model().get(MODE), 3.0, "Esc changes nothing");
}

#[test]
fn the_wheel_steps_through_the_choices() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.wheel(ids::MODE, -1.0);
    assert_eq!(h.model().get(MODE), 1.0);
    h.wheel(ids::MODE, 1.0).wheel(ids::MODE, 1.0);
    assert_eq!(h.model().get(MODE), 0.0, "clamped at the first");
}

#[test]
fn snapshots() {
    let mut h = Harness::new(widgets::editor, 2.0);
    h.wheel(ids::MODE, -2.0);
    h.move_to(Point::new(1.0, 1.0)).advance(16.0);
    assert_snapshot!(h, "dropdown_field@2x");
    h.click(ids::MODE);
    key(&mut h, Key::ArrowDown);
    assert_snapshot!(h, "dropdown_open@2x");
}
