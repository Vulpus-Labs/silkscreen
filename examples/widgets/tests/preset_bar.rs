//! The preset bar: the field's ‹ › stepping, the modified marker,
//! the Save / Save As / Init buttons, the field's flash, and the current
//! preset kept in the UI state.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, MouseButton, Point, RawKind};
use silkscreen::{Folder, MemoryStore, Place, PresetPath, PresetStore};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, LEVEL, faceplate::LAYOUT, faceplate::ids};

#[derive(Clone, Default)]
struct Probe(Rc<RefCell<MemoryStore>>);

impl PresetStore for Probe {
    fn tree(&self) -> Vec<Folder> {
        self.0.borrow().tree()
    }
    fn read(&self, path: &PresetPath) -> Option<String> {
        self.0.borrow().read(path)
    }
    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String> {
        self.0.borrow_mut().write(path, text)
    }
    fn remove(&mut self, path: &PresetPath) -> Result<(), String> {
        self.0.borrow_mut().remove(path)
    }
    fn add_folder(&mut self, name: &str) -> Result<(), String> {
        self.0.borrow_mut().add_folder(name)
    }
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.0.borrow_mut().rename_folder(from, to)
    }
    fn remove_folder(&mut self, name: &str) -> Result<(), String> {
        self.0.borrow_mut().remove_folder(name)
    }
}

/// "Mine" at the root and "Lead A" in "Leads".
fn probe() -> Probe {
    let p = Probe::default();
    {
        let mut s = p.0.borrow_mut();
        s.write(&PresetPath::root("Mine"), "name = Mine\np1 = 0.3\n")
            .unwrap();
        s.write(
            &PresetPath::new(Some("Leads"), "Lead A"),
            "name = Lead A\np2 = 0.2\n",
        )
        .unwrap();
    }
    p
}

fn harness(store: &Probe, scale: f32) -> Harness {
    let store = store.clone();
    Harness::new(
        move |link| widgets::editor_with(link, Box::new(store)),
        scale,
    )
}

fn key(h: &mut Harness, key: Key) {
    h.raw(RawKind::Key {
        key,
        mods: Modifiers::NONE,
        pressed: true,
    });
}

fn typ(h: &mut Harness, text: &str) {
    for c in text.chars() {
        key(h, Key::Char(c));
    }
}

/// A point on the field: its ‹ (-1), name (0) or › (1).
fn field(zone: i8) -> Point {
    let r = LAYOUT.control(ids::PROGRAM).rect;
    let y = r.center().y;
    match zone {
        -1 => Point::new(r.x + 4.0, y),
        1 => Point::new(r.right() - 4.0, y),
        _ => r.center(),
    }
}

fn current(h: &Harness) -> (Option<String>, Option<Place>) {
    let s = h.editor().state();
    (
        s.preset_name().map(str::to_owned),
        s.preset_place().cloned(),
    )
}

fn named(name: &str, place: Place) -> (Option<String>, Option<Place>) {
    (Some(name.to_owned()), Some(place))
}

#[test]
fn the_arrows_step_through_factory_then_user_presets_and_wrap() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let basic = || Place::Factory("Basic");
    h.click_at(field(1));
    assert_eq!(
        current(&h),
        named("Bright Saw", basic()),
        "from none, the first"
    );
    assert_eq!(h.editor().panels(), 0, "an arrow doesn't open the browser");
    h.click_at(field(1));
    assert_eq!(current(&h), named("Init", basic()));
    h.click_at(field(1));
    assert_eq!(current(&h), named("Soft Square", Place::Factory("Mellow")));
    h.click_at(field(1));
    assert_eq!(current(&h), named("Mine", Place::User(None)));
    h.click_at(field(1));
    assert_eq!(
        current(&h),
        named("Lead A", Place::User(Some("Leads".into())))
    );
    assert_eq!(h.model().get(CUTOFF), 0.2);
    h.click_at(field(1));
    assert_eq!(current(&h), named("Bright Saw", basic()), "wraps");
    h.click_at(field(-1));
    assert_eq!(
        current(&h),
        named("Lead A", Place::User(Some("Leads".into()))),
        "and back"
    );

    assert!(h.editor_mut().undo());
    assert_eq!(current(&h).0.as_deref(), Some("Lead A"), "the name stays");
    assert_eq!(
        h.model().get(CUTOFF),
        0.85,
        "one step undone: Bright Saw's sound"
    );

    h.click_at(field(0));
    assert_eq!(h.editor().panels(), 1, "the name opens the browser");
}

#[test]
fn from_nothing_back_is_the_last() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    h.click_at(field(-1));
    assert_eq!(
        current(&h),
        named("Lead A", Place::User(Some("Leads".into())))
    );
}

#[test]
fn user_edits_mark_the_preset_modified_until_saved() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    for _ in 0..4 {
        h.click_at(field(1));
    }
    assert_eq!(current(&h).0.as_deref(), Some("Mine"));
    assert!(
        !h.editor().state().preset_modified(),
        "loading isn't modifying"
    );
    h.host_change(LEVEL, 0.6).advance(16.0);
    assert!(!h.editor().state().preset_modified(), "nor is automation");
    h.click(ids::STOP);
    assert!(
        h.editor().state().preset_modified(),
        "a click on a control is"
    );
    h.click(ids::SAVE);
    assert!(!h.editor().state().preset_modified(), "saved");
    let text = store.read(&PresetPath::root("Mine")).unwrap();
    assert!(text.contains("p1 = 0.6"), "saved in place:\n{text}");
    assert_eq!(h.editor().panels(), 0, "no dialog for a user preset");
}

#[test]
fn save_on_a_factory_preset_asks_for_a_name_and_save_as_saves() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    h.click_at(field(1));
    h.click(ids::SAVE);
    assert_eq!(h.editor().panels(), 1, "Save As for the factory's");
    key(&mut h, Key::Escape);
    h.click(ids::SAVEAS);
    typ(&mut h, "My Saw");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(current(&h), named("My Saw", Place::User(None)));
    assert!(store.read(&PresetPath::root("My Saw")).is_some());
    assert_eq!(
        h.editor().state().preset_flash(),
        Some("Saved \u{201c}My Saw\u{201d}")
    );
    h.advance(1000.0);
    assert!(h.editor().state().preset_flash().is_some());
    h.advance(600.0);
    assert!(
        h.editor().state().preset_flash().is_none(),
        "gone after 1.5 s"
    );
}

#[test]
fn init_resets_and_clears_the_current_preset() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    h.click_at(field(1));
    assert_eq!(h.model().get(CUTOFF), 0.85);
    h.click(ids::INIT);
    assert_eq!(h.model().get(CUTOFF), 0.5);
    assert_eq!(current(&h), (None, None));
    assert!(h.editor_mut().undo());
    assert_eq!(h.model().get(CUTOFF), 0.85, "one undo step");
}

#[test]
fn stepping_with_no_presets_says_so() {
    let mut h = Harness::new(widgets::live_editor, 1.0);
    // The live editor has no preset setup; check the state call directly.
    let e = h.editor_mut().state.step_preset(true).unwrap_err();
    assert!(e.contains("No presets"), "{e}");
}

#[test]
fn the_current_preset_survives_in_the_ui_state() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    for _ in 0..5 {
        h.click_at(field(1));
    }
    h.click(ids::STOP);
    let saved = h.editor().state().ui_state();
    let get = |k: &str| {
        saved
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("preset.name"), Some("Lead A"));
    assert_eq!(get("preset.place"), Some("user:Leads"));
    assert_eq!(get("preset.modified"), Some("1"));

    let mut fresh = harness(&store, 1.0);
    fresh.editor_mut().state.restore_ui_state(&saved);
    assert_eq!(
        current(&fresh),
        named("Lead A", Place::User(Some("Leads".into())))
    );
    assert!(fresh.editor().state().preset_modified());

    // A factory place maps back to the bank's category, if it has it.
    let mut factory = harness(&store, 1.0);
    factory.editor_mut().state.restore_ui_state(&[
        ("preset.name".into(), "Init".into()),
        ("preset.place".into(), "factory:Basic".into()),
    ]);
    assert_eq!(current(&factory), named("Init", Place::Factory("Basic")));
    factory
        .editor_mut()
        .state
        .restore_ui_state(&[("preset.place".into(), "factory:Gone".into())]);
    assert_eq!(current(&factory).1, None);
}

#[test]
fn snapshots() {
    let store = probe();
    let mut h = harness(&store, 2.0);
    h.click_at(field(1));
    h.advance(16.0);
    assert_snapshot!(h, "preset_bar@2x");
    h.raw(RawKind::PointerDown {
        pos: LAYOUT.control(ids::STOP).rect.center(),
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    })
    .raw(RawKind::PointerUp {
        pos: LAYOUT.control(ids::STOP).rect.center(),
        button: MouseButton::Left,
        mods: Modifiers::NONE,
    });
    h.move_to(LAYOUT.control(ids::SAVE).rect.center())
        .advance(16.0);
    assert_snapshot!(h, "preset_bar-modified@2x");
}

/// A plugin's randomiser names the sound and its user folder (design/presets.md):
/// Save As offers that folder, made on save, or an existing one
/// of that name in any case; loading anything else forgets it.
#[test]
fn save_as_offers_a_random_sounds_folder() {
    use silkscreen::random::{RandomSound, Seed};
    let store = probe();
    let mut h = harness(&store, 1.0);
    let folder = Rc::new(RefCell::new("Pads"));
    let f = Rc::clone(&folder);
    h.editor_mut()
        .state
        .set_randomiser(move |_rng, _before, defaults| RandomSound {
            values: defaults.to_vec(),
            name: "Gelid Kettle".to_owned(),
            folder: Some((*f.borrow()).to_owned()),
            comment: Some("Random Pad".to_owned()),
        });
    let seed = Seed::parse("PAD01").unwrap();

    h.editor_mut().state.randomise_preset_seeded(seed);
    assert_eq!(current(&h), (Some("Gelid Kettle".to_owned()), None));
    // The seed is in the comment, which the name field's tooltip shows.
    let comment = Some("Random Pad, seed PAD01");
    assert_eq!(h.editor().state().preset_comment(), comment);
    assert_eq!(
        h.editor().state().describe(ids::PROGRAM).as_deref(),
        comment
    );
    h.click(ids::SAVEAS);
    key(&mut h, Key::Enter);
    assert_eq!(
        current(&h),
        named("Gelid Kettle", Place::User(Some("Pads".into())))
    );
    let saved = store
        .read(&PresetPath::new(Some("Pads"), "Gelid Kettle"))
        .unwrap();
    assert!(
        saved.contains("comment = Random Pad, seed PAD01"),
        "the seed kept:\n{saved}"
    );

    *folder.borrow_mut() = "leads";
    h.editor_mut().state.randomise_preset_seeded(seed);
    h.click(ids::SAVEAS);
    key(&mut h, Key::Enter);
    assert_eq!(
        current(&h),
        named("Gelid Kettle", Place::User(Some("Leads".into()))),
        "the existing folder, whatever its case"
    );

    h.editor_mut().state.randomise_preset_seeded(seed);
    h.click(ids::INIT);
    assert_eq!(h.editor().state().random_folder(), None, "Init forgets it");
    assert_eq!(h.editor().state().preset_comment(), None, "and the comment");
}

#[test]
fn a_restored_state_with_no_comment_clears_the_comment() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    h.editor_mut().state.restore_ui_state(&[
        ("preset.name".into(), "Lead A".into()),
        ("preset.comment".into(), "A bright lead".into()),
    ]);
    assert_eq!(h.editor().state().preset_comment(), Some("A bright lead"));
    h.editor_mut()
        .state
        .restore_ui_state(&[("preset.name".into(), "Init".into())]);
    assert_eq!(
        h.editor().state().preset_comment(),
        None,
        "not the last one's"
    );
}

#[test]
fn a_restored_state_with_no_preset_shows_none() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    for _ in 0..5 {
        h.click_at(field(1));
    }
    h.click(ids::STOP);
    assert!(h.editor().state().preset_modified());
    // A state saved from a fresh editor: no name, place or modification.
    h.editor_mut().state.restore_ui_state(&[]);
    assert_eq!(h.editor().state().preset_name(), None);
    assert_eq!(h.editor().state().preset_place(), None);
    assert!(!h.editor().state().preset_modified());
}

#[test]
fn save_as_with_a_host_prompt_saves_on_its_enter() {
    // A plugin's host prompts for names.
    let store = probe();
    let mut h = harness(&store, 1.0);
    let prompt = silkscreen::prompt::ScriptedPrompt::new();
    h.editor_mut().state.set_text_prompt(prompt.clone());
    h.click(ids::SAVEAS);
    h.advance(16.0);
    assert_eq!(prompt.asked().len(), 1, "asked at once");
    assert_eq!(prompt.asked()[0].0, "Save Preset As");
    assert!(!h.editor().takes_text());

    // A cancel leaves the dialog; a click on the name asks again.
    prompt.answer(None);
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 1, "still open");
    h.advance(16.0);
    assert_eq!(prompt.asked().len(), 1, "not asked again by itself");
    let name = h.editor().top_panel_rect().unwrap();
    h.click_at(Point::new(name.x + 120.0, name.y + 22.0));
    h.advance(16.0);
    assert_eq!(prompt.asked().len(), 2, "the name, clicked");

    prompt.answer(Some("My Pad"));
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 0);
    assert_eq!(current(&h), named("My Pad", Place::User(None)));
    assert!(store.read(&PresetPath::root("My Pad")).is_some());
}
