//! The Unregistered strip and the unlock dialog (design/licensing-hook.md),
//! against a stand-in for the plugin's lock.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, MouseCursor, RawKind};
use silkscreen::prompt::ScriptedPrompt;
use silkscreen::{Clipboard, Licence, Unlock};
use silkscreen_harness::{Harness, assert_snapshot};

const GOOD: &str = "ACME1-GOOD";

/// A lock that one code opens, shared like the plugin's process-wide flag.
#[derive(Clone)]
struct Lock(Rc<Cell<bool>>);

impl Licence for Lock {
    fn locked(&self) -> bool {
        self.0.get()
    }

    fn unlock(&self, code: &str) -> Result<(), String> {
        if code.trim() == GOOD {
            self.0.set(false);
            Ok(())
        } else {
            Err("That code is not valid. Check that it was copied exactly.".into())
        }
    }
}

#[derive(Clone, Default)]
struct Board(Rc<RefCell<Option<String>>>);

impl Clipboard for Board {
    fn text(&self) -> Option<String> {
        self.0.borrow().clone()
    }
}

fn harness(lock: &Lock, scale: f32) -> (Harness, Board) {
    let mut h = Harness::new(widgets::editor, scale);
    let board = Board::default();
    h.editor_mut().state.set_licence(Rc::new(lock.clone()));
    h.editor_mut().state.set_clipboard(Rc::new(board.clone()));
    h.advance(16.0);
    (h, board)
}

fn locked() -> Lock {
    Lock(Rc::new(Cell::new(true)))
}

fn key(h: &mut Harness, key: Key, mods: Modifiers) {
    h.raw(RawKind::Key {
        key,
        mods,
        pressed: true,
    });
}

fn typ(h: &mut Harness, text: &str) {
    for c in text.chars() {
        key(h, Key::Char(c), Modifiers::NONE);
    }
}

/// The dialog the strip opens, for its geometry.
fn geometry() -> Unlock {
    Unlock::centred(widgets::faceplate::LAYOUT.size)
}

fn open_dialog(h: &mut Harness) {
    let strip = h.editor().strip_rect().expect("the strip shows");
    h.click_at(strip.center());
    assert_eq!(h.editor().panels(), 1, "the strip opens the dialog");
}

#[test]
fn no_strip_without_a_licence_or_once_unlocked() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.advance(16.0);
    assert_eq!(h.editor().strip_rect(), None, "an unlicensed editor");

    let lock = Lock(Rc::new(Cell::new(false)));
    let (h, _) = harness(&lock, 1.0);
    assert_eq!(h.editor().strip_rect(), None, "unlocked");
}

#[test]
fn the_strip_sits_bottom_right_and_opens_the_dialog() {
    let lock = locked();
    let (mut h, _) = harness(&lock, 1.0);
    let strip = h.editor().strip_rect().unwrap();
    let size = widgets::faceplate::LAYOUT.size;
    assert!(strip.right() <= size.w && strip.bottom() <= size.h);
    assert!(strip.x > size.w / 2.0 && strip.y > size.h / 2.0);
    h.move_to(strip.center());
    assert_eq!(h.cursor(), MouseCursor::Hand);
    open_dialog(&mut h);
    assert!(h.editor().takes_text(), "the field takes keys");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(h.editor().panels(), 0, "Esc cancels");
    assert!(lock.locked());
}

#[test]
fn a_refused_code_shows_why_and_stays_in_the_field() {
    let lock = locked();
    let (mut h, _) = harness(&lock, 1.0);
    open_dialog(&mut h);
    typ(&mut h, "ACME1-BAD");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.editor().panels(), 1, "still open");
    assert!(lock.locked());

    // Fix it up and press Unlock.
    for _ in 0.."BAD".len() {
        key(&mut h, Key::Backspace, Modifiers::NONE);
    }
    typ(&mut h, "GOOD");
    h.click_at(geometry().button_rect(2).center());
    assert_eq!(h.editor().panels(), 0, "a good code closes the dialog");
    assert!(!lock.locked());
    h.advance(16.0);
    assert_eq!(h.editor().strip_rect(), None, "and the strip goes");
}

#[test]
fn paste_fills_the_field_and_enter_unlocks() {
    let lock = locked();
    let (mut h, board) = harness(&lock, 1.0);
    open_dialog(&mut h);
    h.click_at(geometry().button_rect(0).center());
    assert_eq!(h.editor().panels(), 1, "nothing to paste: still open");

    *board.0.borrow_mut() = Some(format!("  {GOOD}\n"));
    h.click_at(geometry().button_rect(0).center());
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(!lock.locked(), "the pasted code unlocks");
}

#[test]
fn cmd_v_pastes_into_the_field() {
    let lock = locked();
    let (mut h, board) = harness(&lock, 1.0);
    *board.0.borrow_mut() = Some(GOOD.into());
    open_dialog(&mut h);
    let cmd = Modifiers {
        cmd: true,
        ..Modifiers::NONE
    };
    key(&mut h, Key::Char('v'), cmd);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(!lock.locked());
}

#[test]
fn unlocking_in_one_editor_clears_the_strip_in_every_editor() {
    let lock = locked();
    let (mut a, _) = harness(&lock, 1.0);
    let (mut b, _) = harness(&lock, 1.0);
    assert!(b.editor().strip_rect().is_some());
    open_dialog(&mut a);
    typ(&mut a, GOOD);
    key(&mut a, Key::Enter, Modifiers::NONE);
    b.advance(16.0);
    assert_eq!(b.editor().strip_rect(), None);
}

#[test]
fn with_a_host_prompt_the_field_asks_it_and_its_enter_unlocks() {
    let lock = locked();
    let (mut h, board) = harness(&lock, 1.0);
    let prompt = ScriptedPrompt::new();
    h.editor_mut().state.set_text_prompt(prompt.clone());
    open_dialog(&mut h);
    h.click_at(geometry().code_rect().center());
    assert_eq!(prompt.asked(), [("Licence Code".into(), String::new())]);
    assert!(!h.editor().takes_text(), "the prompt has the keys");

    prompt.answer(Some("wrong"));
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 1, "refused: still open");
    assert!(lock.locked());

    // Paste still works beside the prompt.
    *board.0.borrow_mut() = Some(GOOD.into());
    h.click_at(geometry().button_rect(0).center());
    h.click_at(geometry().code_rect().center());
    assert_eq!(
        prompt.asked()[1].1,
        GOOD,
        "the prompt starts from the field"
    );
    prompt.answer(Some(GOOD));
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 0);
    assert!(!lock.locked());
}

#[test]
fn snapshots() {
    let lock = locked();
    let (mut h, _) = harness(&lock, 2.0);
    assert_snapshot!(h, "licence-strip@2x");
    open_dialog(&mut h);
    assert_snapshot!(h, "licence-unlock@2x");
    typ(
        &mut h,
        "ACME1-0A1B2C3D-4E5F6G7H-8J9KAMBN-CPDQERSF-TGVHWJXK-YMZN0P1Q",
    );
    assert_snapshot!(h, "licence-unlock-filled@2x");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_snapshot!(h, "licence-unlock-error@2x");
}
