//! Editing from the preset browser: menus (right-click and ⋯),
//! rename, move, delete with confirmation, drag to a folder, and the
//! footer's Init, Save and Save As.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, MouseButton, Point, RawKind, Rect};
use silkscreen::list::ROW_H;
use silkscreen::{Folder, MemoryStore, Place, PresetBrowser, PresetPath, PresetStore};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, LEVEL, faceplate::ids};

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

impl Probe {
    /// Every preset's path, as `folder/name` or `name`.
    fn paths(&self) -> Vec<String> {
        self.tree()
            .into_iter()
            .flat_map(|f| {
                f.presets
                    .into_iter()
                    .map(move |n| PresetPath::new(f.name.as_deref(), &n).to_string())
            })
            .collect()
    }

    fn folders(&self) -> Vec<String> {
        self.tree().into_iter().filter_map(|f| f.name).collect()
    }
}

/// "Mine" at the root; "Lead A" and "Lead B" in "Leads".
fn probe() -> Probe {
    let p = Probe::default();
    {
        let mut s = p.0.borrow_mut();
        s.write(&PresetPath::root("Mine"), "name = Mine\np1 = 0.3\n")
            .unwrap();
        for (name, cutoff) in [("Lead A", 0.2), ("Lead B", 0.7)] {
            s.write(
                &PresetPath::new(Some("Leads"), name),
                &format!("name = {name}\np2 = {cutoff}\n"),
            )
            .unwrap();
        }
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

fn rect() -> Rect {
    PresetBrowser::centred(widgets::faceplate::LAYOUT.size)
}

/// A browser like the one the field opens, for its geometry.
fn geometry(h: &Harness) -> PresetBrowser {
    PresetBrowser::new(
        rect(),
        Box::new(MemoryStore::default()),
        widgets::FACTORY,
        h.editor().state(),
    )
}

fn row(pane: Rect, i: usize) -> Point {
    Point::new(pane.x + 30.0, pane.y + 3.0 + (i as f32 + 0.5) * ROW_H)
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

fn right_click(h: &mut Harness, pos: Point) {
    for down in [true, false] {
        let button = MouseButton::Right;
        let mods = Modifiers::NONE;
        h.raw(if down {
            RawKind::PointerDown { pos, button, mods }
        } else {
            RawKind::PointerUp { pos, button, mods }
        });
    }
}

/// Choose menu item `i` (the menu is the top panel) by keys.
fn choose(h: &mut Harness, i: usize) {
    for _ in 0..=i {
        key(h, Key::ArrowDown);
    }
    key(h, Key::Enter);
}

const UNCATEGORISED: usize = 4;
const LEADS: usize = 5;

/// Open the browser and show Leads.
fn open_leads(h: &mut Harness) -> PresetBrowser {
    let b = geometry(h);
    h.click(ids::PROGRAM);
    h.click_at(row(b.places_rect(), LEADS));
    b
}

#[test]
fn rename_from_the_menu_and_a_taken_name_keeps_the_field() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    right_click(&mut h, row(b.presets_rect(), 0));
    assert_eq!(h.editor().panels(), 2, "the menu");
    choose(&mut h, 0);
    assert_eq!(h.editor().panels(), 2, "the menu became a name field");
    typ(&mut h, "Lead B");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 2, "taken: still naming");
    key(&mut h, Key::Escape);
    right_click(&mut h, row(b.presets_rect(), 0));
    choose(&mut h, 0);
    typ(&mut h, "Lead C");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1);
    assert_eq!(store.paths(), ["Mine", "Leads/Lead B", "Leads/Lead C"]);
}

#[test]
fn renaming_the_current_preset_renames_the_field() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    h.click_at(row(b.presets_rect(), 0));
    assert_eq!(h.editor().state().preset_name(), Some("Lead A"));
    right_click(&mut h, row(b.presets_rect(), 0));
    choose(&mut h, 0);
    typ(&mut h, "Bright Lead");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().state().preset_name(), Some("Bright Lead"));
}

#[test]
fn move_to_a_folder_from_the_menu_and_follow_it() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    h.click_at(row(b.presets_rect(), 0));
    right_click(&mut h, row(b.presets_rect(), 0));
    choose(&mut h, 1);
    assert_eq!(h.editor().panels(), 2, "the folders menu");
    choose(&mut h, 0);
    assert_eq!(h.editor().panels(), 1);
    assert_eq!(store.paths(), ["Lead A", "Mine", "Leads/Lead B"]);
    assert_eq!(
        h.editor().state().preset_place(),
        Some(&Place::User(None)),
        "the current preset moved with it"
    );
    // The browser followed: its first row is now the root's "Lead A".
    h.click_at(row(b.presets_rect(), 1));
    assert_eq!(h.editor().state().preset_name(), Some("Mine"));
}

#[test]
fn delete_asks_first() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    right_click(&mut h, row(b.presets_rect(), 1));
    choose(&mut h, 2);
    assert_eq!(h.editor().panels(), 2, "Delete?");
    key(&mut h, Key::Escape);
    assert_eq!(store.paths().len(), 3, "cancelled");
    right_click(&mut h, row(b.presets_rect(), 1));
    choose(&mut h, 2);
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1);
    assert_eq!(store.paths(), ["Mine", "Leads/Lead A"]);
}

#[test]
fn factory_rows_and_the_root_have_no_menu() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = geometry(&h);
    h.click(ids::PROGRAM);
    right_click(&mut h, row(b.presets_rect(), 0));
    assert_eq!(h.editor().panels(), 1, "Bright Saw is the factory's");
    right_click(&mut h, row(b.places_rect(), 1));
    assert_eq!(h.editor().panels(), 1, "nor its categories");
    right_click(&mut h, row(b.places_rect(), UNCATEGORISED));
    assert_eq!(h.editor().panels(), 1, "nor the root");
}

#[test]
fn the_dots_open_the_menu_too() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    let dots = PresetBrowser::dots_rect(b.preset_row(0)).center();
    h.move_to(dots);
    h.click_at(dots);
    assert_eq!(h.editor().panels(), 2, "the ⋯ opened the menu");
    assert_eq!(h.editor().state().preset_name(), None, "and loaded nothing");
}

#[test]
fn a_folder_menu_renames_and_deletes_with_its_presets() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    right_click(&mut h, row(b.places_rect(), LEADS));
    choose(&mut h, 0);
    typ(&mut h, "Solo");
    key(&mut h, Key::Enter);
    assert_eq!(store.folders(), ["Solo"]);
    right_click(&mut h, row(b.places_rect(), LEADS));
    choose(&mut h, 1);
    key(&mut h, Key::Enter);
    assert!(store.folders().is_empty());
    assert_eq!(store.paths(), ["Mine"], "its presets went with it");
}

#[test]
fn dragging_a_preset_onto_a_folder_moves_it() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    let from = row(b.presets_rect(), 1);
    let to = row(b.places_rect(), UNCATEGORISED);
    let mods = Modifiers::NONE;
    let button = MouseButton::Left;
    h.move_to(from)
        .raw(RawKind::PointerDown {
            pos: from,
            button,
            mods,
        })
        .raw(RawKind::PointerMove { pos: to, mods })
        .raw(RawKind::PointerUp {
            pos: to,
            button,
            mods,
        });
    assert_eq!(store.paths(), ["Lead B", "Mine", "Leads/Lead A"]);
    assert_eq!(
        h.editor().state().preset_name(),
        None,
        "a drag doesn't load"
    );
}

#[test]
fn save_as_saves_and_asks_before_replacing() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = geometry(&h);
    h.click(ids::PROGRAM);
    h.click_at(b.footer_button(3).center());
    assert_eq!(h.editor().panels(), 2, "the dialog");
    typ(&mut h, "Pad 1");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1);
    assert_eq!(
        store.paths(),
        ["Mine", "Pad 1", "Leads/Lead A", "Leads/Lead B"]
    );
    assert_eq!(h.editor().state().preset_name(), Some("Pad 1"));
    assert_eq!(h.editor().state().preset_place(), Some(&Place::User(None)));

    h.click_at(b.footer_button(3).center());
    typ(&mut h, "Mine");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 2, "Replace?");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1);
    let text = store.read(&PresetPath::root("Mine")).unwrap();
    assert!(
        text.contains("p1 = 0.8"),
        "replaced with the current sound:\n{text}"
    );

    h.click_at(b.footer_button(3).center());
    typ(&mut h, "a/b");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 2, "a bad name keeps the dialog");
}

#[test]
fn save_overwrites_a_user_preset_and_is_save_as_for_the_factorys() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    h.click_at(row(b.presets_rect(), 0));
    h.host_change(LEVEL, 0.25).advance(16.0);
    h.click_at(b.footer_button(2).center());
    assert_eq!(h.editor().panels(), 1, "saved in place");
    let text = store
        .read(&PresetPath::new(Some("Leads"), "Lead A"))
        .unwrap();
    assert!(text.contains("p1 = 0.25"), "{text}");

    h.click_at(row(b.places_rect(), 1));
    h.click_at(row(b.presets_rect(), 0));
    h.click_at(b.footer_button(2).center());
    assert_eq!(h.editor().panels(), 2, "a factory preset saves as");
}

#[test]
fn init_resets_every_param_as_one_step() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    h.click_at(row(b.presets_rect(), 1));
    assert_eq!(h.model().get(CUTOFF), 0.7);
    h.click_at(b.footer_button(1).center());
    assert_eq!(h.model().get(CUTOFF), 0.5);
    assert_eq!(
        h.editor().state().preset_name(),
        None,
        "the field shows Init"
    );
    key(&mut h, Key::Escape);
    assert!(h.editor_mut().undo());
    assert_eq!(h.model().get(CUTOFF), 0.7, "one undo step");
}

#[test]
fn esc_closes_one_level_at_a_time() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = open_leads(&mut h);
    right_click(&mut h, row(b.presets_rect(), 0));
    choose(&mut h, 2);
    assert_eq!(h.editor().panels(), 2);
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 1, "the dialog");
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 0, "then the browser");
}

#[test]
fn snapshots() {
    let store = probe();
    let mut h = harness(&store, 2.0);
    let b = open_leads(&mut h);
    let at = row(b.presets_rect(), 0);
    h.move_to(at);
    assert_snapshot!(h, "browser-hover-dots@2x");
    right_click(&mut h, at);
    assert_snapshot!(h, "browser-menu@2x");
    key(&mut h, Key::Escape);

    h.click_at(b.footer_button(3).center());
    assert_snapshot!(h, "browser-save-as@2x");
    key(&mut h, Key::Escape);

    let from = row(b.presets_rect(), 1);
    let to = row(b.places_rect(), UNCATEGORISED);
    let mods = Modifiers::NONE;
    h.move_to(from)
        .raw(RawKind::PointerDown {
            pos: from,
            button: MouseButton::Left,
            mods,
        })
        .raw(RawKind::PointerMove { pos: to, mods });
    assert_snapshot!(h, "browser-drag@2x");
}

#[test]
fn only_the_browser_and_name_fields_take_typed_text() {
    // In a plugin, keys reach the editor only while it takes text; the
    // rest go to the DAW.
    let store = probe();
    let mut h = harness(&store, 1.0);
    assert!(!h.editor().takes_text(), "the faceplate");
    let b = open_leads(&mut h);
    assert!(h.editor().takes_text(), "the browser's search");
    right_click(&mut h, row(b.presets_rect(), 0));
    assert!(!h.editor().takes_text(), "a menu");
    choose(&mut h, 0);
    assert!(h.editor().takes_text(), "a name field");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 0);
    assert!(!h.editor().takes_text(), "closed");
}

/// A harness whose host prompts for names (a plugin's).
fn prompted(store: &Probe) -> (Harness, Rc<silkscreen::prompt::ScriptedPrompt>) {
    let mut h = harness(store, 1.0);
    let prompt = silkscreen::prompt::ScriptedPrompt::new();
    h.editor_mut().state.set_text_prompt(prompt.clone());
    (h, prompt)
}

#[test]
fn a_host_prompt_renames_and_asks_again_for_a_taken_name() {
    let store = probe();
    let (mut h, prompt) = prompted(&store);
    let b = open_leads(&mut h);
    h.advance(16.0);
    assert!(prompt.asked().is_empty(), "search stays a field");
    assert!(h.editor().takes_text(), "the search takes keys");
    right_click(&mut h, row(b.presets_rect(), 0));
    choose(&mut h, 0);
    h.advance(16.0);
    assert_eq!(
        prompt.asked(),
        [("Preset Name".into(), "Lead A".into())],
        "the name field asks the host"
    );
    assert!(!h.editor().takes_text(), "and takes no keys");

    // A taken name: the error shows and the host is asked again.
    prompt.answer(Some("Lead B"));
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 2, "still naming");
    h.advance(16.0);
    assert_eq!(prompt.asked().len(), 2);
    assert_eq!(prompt.asked()[1].1, "Lead B", "with what was typed");

    prompt.answer(Some("Lead C"));
    h.advance(16.0);
    assert_eq!(h.editor().panels(), 1, "renamed");
    assert!(store.paths().contains(&"Leads/Lead C".to_owned()));
    assert_eq!(prompt.dismissed(), 0);
}

#[test]
fn cancelling_a_host_prompt_closes_the_field_and_closing_dismisses_it() {
    let store = probe();
    let (mut h, prompt) = prompted(&store);
    let b = open_leads(&mut h);
    right_click(&mut h, row(b.places_rect(), LEADS));
    choose(&mut h, 0);
    h.advance(16.0);
    assert_eq!(prompt.asked(), [("Folder Name".into(), "Leads".into())]);
    prompt.answer(None);
    h.advance(16.0);
    assert_eq!(
        h.editor().panels(),
        1,
        "the field closed; the browser stays"
    );

    // Closing the field with its prompt open closes the prompt.
    right_click(&mut h, row(b.places_rect(), LEADS));
    choose(&mut h, 0);
    h.advance(16.0);
    key(&mut h, Key::Escape);
    assert_eq!(prompt.dismissed(), 1);
}
