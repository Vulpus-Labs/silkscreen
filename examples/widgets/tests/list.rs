//! The scrolling list panel.

use std::cell::Cell;
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, MouseButton, Point, RawKind, Rect, Vec2};
use silkscreen::list::ROW_H;
use silkscreen::{List, Reply};
use silkscreen_harness::{Harness, assert_snapshot};

/// 40 rows in a 120-unit-tall panel: about six visible.
const PANEL: Rect = Rect::new(140.0, 20.0, 130.0, 120.0);

fn open(h: &mut Harness, close_on_choose: bool) -> Rc<Cell<Option<usize>>> {
    let chosen = Rc::new(Cell::new(None));
    let seen = Rc::clone(&chosen);
    let items = (1..=40).map(|i| format!("Preset {i:02}")).collect();
    let list = List::new(PANEL, items, move |i, _| {
        seen.set(Some(i));
        if close_on_choose {
            Reply::Close
        } else {
            Reply::Handled
        }
    })
    .with_selected(0);
    h.editor_mut().open_panel(Box::new(list));
    chosen
}

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

fn wheel(h: &mut Harness, lines: f32) {
    h.raw(RawKind::Wheel {
        pos: PANEL.center(),
        delta: Vec2::new(0.0, lines),
        mods: Modifiers::NONE,
    });
}

/// The y of row `i`'s middle when scrolled by `scroll`.
fn row_y(i: usize, scroll: f32) -> f32 {
    PANEL.y + 3.0 + i as f32 * ROW_H - scroll + ROW_H / 2.0
}

#[test]
fn clicking_a_row_chooses_it() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let chosen = open(&mut h, true);
    h.click_at(Point::new(PANEL.x + 30.0, row_y(2, 0.0)));
    assert_eq!(chosen.get(), Some(2));
    assert_eq!(h.editor().panels(), 0, "closed on choice");
}

#[test]
fn wheel_and_keys_scroll_and_keep_the_selection_in_view() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let chosen = open(&mut h, false);
    wheel(&mut h, -3.0);
    // Three rows down: row 3 is now at the top.
    h.click_at(Point::new(PANEL.x + 30.0, row_y(0, 0.0)));
    assert_eq!(chosen.get(), Some(3));

    key(&mut h, Key::End);
    key(&mut h, Key::Enter);
    assert_eq!(chosen.get(), Some(39), "End selects the last row");
    key(&mut h, Key::Home);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_eq!(chosen.get(), Some(2));
    assert_eq!(h.editor().panels(), 1, "this list stays open");
}

#[test]
fn dragging_the_thumb_scrolls_to_the_end() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let chosen = open(&mut h, false);
    // The thumb starts at the top of the track, right of the rows.
    let thumb = Point::new(PANEL.right() - 6.0, PANEL.y + 8.0);
    h.move_to(thumb).raw(RawKind::PointerDown {
        pos: thumb,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    let end = Point::new(thumb.x, PANEL.bottom() + 40.0);
    h.raw(RawKind::PointerMove {
        pos: end,
        mods: Modifiers::NONE,
    })
    .raw(RawKind::PointerUp {
        pos: end,
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    assert_eq!(
        h.editor().panels(),
        1,
        "a drag past the panel doesn't close it"
    );
    // Scrolled to the end: the last visible row is the last one.
    h.click_at(Point::new(PANEL.x + 30.0, PANEL.bottom() - 6.0));
    assert_eq!(chosen.get(), Some(39));
}

#[test]
fn snapshot() {
    let mut h = Harness::new(widgets::editor, 2.0);
    open(&mut h, false);
    key(&mut h, Key::ArrowDown);
    wheel(&mut h, -0.5);
    h.move_to(Point::new(PANEL.x + 30.0, PANEL.y + 60.0));
    assert_snapshot!(h, "list@2x");
}
