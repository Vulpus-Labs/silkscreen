//! Single-line text entry.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, Point, RawKind, Rect};
use silkscreen::{Reply, TextField};
use silkscreen_harness::{Harness, assert_snapshot};

const FIELD: Rect = Rect::new(140.0, 60.0, 120.0, 20.0);

fn open(h: &mut Harness, text: &str, max: usize) -> Rc<RefCell<Option<String>>> {
    let committed = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&committed);
    let field = TextField::new(FIELD, text, move |t, _| {
        *seen.borrow_mut() = Some(t.to_owned());
        Reply::Close
    })
    .with_max_chars(max);
    h.editor_mut().open_panel(Box::new(field));
    committed
}

fn press(h: &mut Harness, key: Key, mods: Modifiers) {
    h.raw(RawKind::Key {
        key,
        mods,
        pressed: true,
    });
}

fn typ(h: &mut Harness, text: &str) {
    for c in text.chars() {
        press(h, Key::Char(c), Modifiers::NONE);
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::NONE
    }
}

#[test]
fn typing_replaces_the_initial_selection_and_enter_commits() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let committed = open(&mut h, "Init", 64);
    typ(&mut h, "Squelch 1");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(committed.borrow().as_deref(), Some("Squelch 1"));
    assert_eq!(h.editor().panels(), 0);
}

#[test]
fn editing_keys_and_selection() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let committed = open(&mut h, "Bass", 64);
    press(&mut h, Key::End, Modifiers::NONE);
    typ(&mut h, " 02");
    press(&mut h, Key::Backspace, Modifiers::NONE);
    typ(&mut h, "4"); // "Bass 04"
    press(&mut h, Key::Home, Modifiers::NONE);
    press(&mut h, Key::ArrowRight, shift());
    press(&mut h, Key::ArrowRight, shift());
    typ(&mut h, "Gr"); // "Grss 04"
    press(&mut h, Key::ArrowRight, Modifiers::NONE);
    press(&mut h, Key::Delete, Modifiers::NONE); // "Grs 04"
    press(
        &mut h,
        Key::Char('a'),
        Modifiers {
            cmd: true,
            ..Modifiers::NONE
        },
    );
    press(&mut h, Key::ArrowRight, Modifiers::NONE); // collapse to the end
    typ(&mut h, "!");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(committed.borrow().as_deref(), Some("Grs 04!"));
}

#[test]
fn length_is_capped_and_escape_cancels() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let committed = open(&mut h, "", 4);
    typ(&mut h, "abcdef");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(committed.borrow().as_deref(), Some("abcd"));

    let committed = open(&mut h, "Keep", 64);
    typ(&mut h, "Lost");
    press(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(*committed.borrow(), None, "Esc commits nothing");
}

#[test]
fn a_click_places_the_caret() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let committed = open(&mut h, "Pad", 64);
    // Left of the text: the start.
    h.click_at(Point::new(FIELD.x + 2.0, FIELD.center().y));
    typ(&mut h, "Warm ");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(committed.borrow().as_deref(), Some("Warm Pad"));
}

#[test]
fn digits_reach_the_field() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let committed = open(&mut h, "", 64);
    typ(&mut h, "12345");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(committed.borrow().as_deref(), Some("12345"));
}

#[test]
fn snapshot() {
    let mut h = Harness::new(widgets::editor, 2.0);
    open(&mut h, "Squelchy Bass 04", 64);
    press(&mut h, Key::End, Modifiers::NONE);
    for _ in 0..2 {
        press(&mut h, Key::ArrowLeft, shift());
    }
    assert_snapshot!(h, "textfield@2x");
}
