//! The preset field opens the browser and shows the current preset
//!.

use silkscreen::core::{Key, Modifiers, RawKind};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, LEVEL, faceplate::ids};

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

#[test]
fn the_field_opens_the_browser_and_loading_is_one_undo_step() {
    let mut h = Harness::new(widgets::editor, 1.0);
    h.click(ids::PROGRAM);
    assert_eq!(h.editor().panels(), 1, "the browser opened");
    // Nothing selected yet: Down selects the first, "Bright Saw" (the
    // bank is sorted by name).
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_eq!(h.model().get(CUTOFF), 0.85);
    assert_eq!(h.model().get(LEVEL), 1.0);
    assert_eq!(h.editor().state().preset_name(), Some("Bright Saw"));

    key(&mut h, Key::Escape);
    assert!(h.editor_mut().undo());
    assert_eq!(
        (h.model().get(CUTOFF), h.model().get(LEVEL)),
        (0.5, 0.8),
        "the whole load undone at once"
    );
}

#[test]
fn snapshots() {
    let mut h = Harness::new(widgets::editor, 2.0);
    h.click(ids::PROGRAM);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_snapshot!(h, "preset_browser@2x");
    key(&mut h, Key::Escape);
    h.advance(16.0);
    assert_snapshot!(h, "preset_field@2x");
}
