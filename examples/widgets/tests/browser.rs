//! The two-pane preset browser: places on the left, presets on
//! the right, search, single-click load, New Folder, and closing.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{Key, Modifiers, Point, RawKind, Rect};
use silkscreen::list::ROW_H;
use silkscreen::{Folder, MemoryStore, Place, PresetBrowser, PresetPath, PresetStore};
use silkscreen_harness::{Harness, assert_factory_bank, assert_snapshot};
use widgets::{CUTOFF, LEVEL, faceplate::ids};

/// A store the test can look into, with a settable problem.
#[derive(Clone, Default)]
struct Probe {
    store: Rc<RefCell<MemoryStore>>,
    problem: Rc<RefCell<Option<String>>>,
}

impl PresetStore for Probe {
    fn tree(&self) -> Vec<Folder> {
        self.store.borrow().tree()
    }
    fn read(&self, path: &PresetPath) -> Option<String> {
        self.store.borrow().read(path)
    }
    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String> {
        self.store.borrow_mut().write(path, text)
    }
    fn remove(&mut self, path: &PresetPath) -> Result<(), String> {
        self.store.borrow_mut().remove(path)
    }
    fn add_folder(&mut self, name: &str) -> Result<(), String> {
        self.store.borrow_mut().add_folder(name)
    }
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.store.borrow_mut().rename_folder(from, to)
    }
    fn remove_folder(&mut self, name: &str) -> Result<(), String> {
        self.store.borrow_mut().remove_folder(name)
    }
    fn problem(&self) -> Option<String> {
        self.problem.borrow().clone()
    }
}

/// A store with "Mine" at the root and "Lead A" in "Leads".
fn probe() -> Probe {
    let p = Probe::default();
    let mut s = p.store.borrow_mut();
    s.write(&PresetPath::root("Mine"), "name = Mine\np1 = 0.3\n")
        .unwrap();
    s.write(
        &PresetPath::new(Some("Leads"), "Lead A"),
        "name = Lead A\np2 = 0.2\n",
    )
    .unwrap();
    drop(s);
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

/// A browser like the one the field opens, to measure and to read.
fn browser(h: &Harness, store: &Probe) -> PresetBrowser {
    PresetBrowser::new(
        rect(),
        Box::new(store.clone()),
        widgets::FACTORY,
        h.editor().state(),
    )
}

/// The centre of row `i` in a pane.
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

const UNCATEGORISED: usize = 4;
const LEADS: usize = 5;
const NEW_FOLDER: usize = 6;

#[test]
fn it_lists_places_and_opens_on_the_current_presets() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = browser(&h, &store);
    assert_eq!(
        b.place_labels(),
        [
            "FACTORY",
            "   Basic",
            "   Mellow",
            "USER",
            "   Uncategorised",
            "   Leads",
            "   + New Folder"
        ]
    );
    assert_eq!(
        b.place(),
        &Place::Factory("Basic"),
        "nothing loaded: the first category"
    );
    assert_eq!(b.preset_labels(), ["Bright Saw", "Init"]);

    // Load "Lead A", then a fresh browser opens on its folder.
    h.click(ids::PROGRAM);
    h.click_at(row(b.places_rect(), LEADS));
    h.click_at(row(b.presets_rect(), 0));
    key(&mut h, Key::Escape);
    let b = browser(&h, &store);
    assert_eq!(b.place(), &Place::User(Some("Leads".into())));
    assert_eq!(b.preset_labels(), ["Lead A"]);
}

#[test]
fn choosing_a_place_then_a_preset_loads_it_as_one_step() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = browser(&h, &store);
    h.click(ids::PROGRAM);
    assert_eq!(h.editor().panels(), 1);
    h.click_at(row(b.places_rect(), LEADS));
    h.click_at(row(b.presets_rect(), 0));
    assert_eq!(h.model().get(CUTOFF), 0.2);
    assert_eq!(h.editor().state().preset_name(), Some("Lead A"));
    assert_eq!(h.editor().panels(), 1, "the browser stays open");
    // Headers aren't places.
    h.click_at(row(b.places_rect(), 0));
    h.click_at(row(b.presets_rect(), 0));
    assert_eq!(h.model().get(CUTOFF), 0.2, "still Leads' list");

    key(&mut h, Key::Escape);
    assert!(h.editor_mut().undo());
    assert_eq!(h.model().get(CUTOFF), 0.5, "the load undone at once");
}

#[test]
fn search_finds_presets_in_every_place() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = browser(&h, &store);
    h.click(ids::PROGRAM);
    typ(&mut h, "LEAD");
    // The only hit, from another place; Down selects it, Enter loads it.
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().state().preset_name(), Some("Lead A"));
    // Choosing a place clears the search.
    h.click_at(row(b.places_rect(), UNCATEGORISED));
    h.click_at(row(b.presets_rect(), 0));
    assert_eq!(h.editor().state().preset_name(), Some("Mine"));
    assert_eq!(h.model().get(LEVEL), 0.3);
}

#[test]
fn the_x_esc_and_a_click_outside_close_it() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = browser(&h, &store);
    h.click(ids::PROGRAM);
    h.click_at(b.close_rect().center());
    assert_eq!(h.editor().panels(), 0, "×");
    h.click(ids::PROGRAM);
    key(&mut h, Key::Escape);
    assert_eq!(h.editor().panels(), 0, "Esc");
    h.click(ids::PROGRAM);
    h.click_at(Point::new(rect().x - 4.0, rect().y + 10.0));
    assert_eq!(h.editor().panels(), 0, "outside");
}

#[test]
fn new_folder_makes_one_and_names_it() {
    let store = probe();
    let mut h = harness(&store, 1.0);
    let b = browser(&h, &store);
    h.click(ids::PROGRAM);
    h.click_at(row(b.places_rect(), NEW_FOLDER));
    assert_eq!(h.editor().panels(), 2, "a name field over the new row");
    typ(&mut h, "Pads");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 1, "named");
    let folders = |store: &Probe| -> Vec<Option<String>> {
        store.tree().into_iter().map(|f| f.name).collect()
    };
    assert_eq!(
        folders(&store),
        [None, Some("Leads".into()), Some("Pads".into())]
    );

    // A taken name keeps the field open, and the folder keeps its name.
    h.click_at(row(b.places_rect(), NEW_FOLDER + 1));
    typ(&mut h, "Leads");
    key(&mut h, Key::Enter);
    assert_eq!(h.editor().panels(), 2, "still naming");
    key(&mut h, Key::Escape);
    assert_eq!(
        folders(&store),
        [
            None,
            Some("Leads".into()),
            Some("New Folder".into()),
            Some("Pads".into())
        ]
    );
}

#[test]
fn snapshots() {
    let store = probe();
    let mut h = harness(&store, 2.0);
    h.click(ids::PROGRAM);
    assert_snapshot!(h, "browser@2x");
    // Searching, with the store's problem in the footer.
    *store.problem.borrow_mut() = Some("Couldn't save presets: QuotaExceededError".into());
    typ(&mut h, "i");
    assert_snapshot!(h, "browser-search@2x");
}

#[test]
fn the_widgets_factory_bank_is_sound() {
    assert_factory_bank(widgets::FACTORY, &widgets::PARAMS);
    let categories: Vec<&str> = widgets::FACTORY.iter().map(|p| p.category).collect();
    assert_eq!(categories, ["Basic", "Basic", "Mellow"]);
}
