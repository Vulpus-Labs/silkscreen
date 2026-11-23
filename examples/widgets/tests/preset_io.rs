//! Export, Import and Share in the browser's footer, and preset files
//! dropped on the editor, with libraries of presets,
//! against a stand-in for a host's `PresetIo`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use silkscreen::core::{DropPayload, Point, RawKind};
use silkscreen::list::ROW_H;
use silkscreen::{IoEvent, MemoryStore, PresetBrowser, PresetIo, PresetPath, PresetStore};
use silkscreen_harness::{Harness, assert_snapshot};
use widgets::{CUTOFF, faceplate::ids};

#[derive(Default)]
struct FakeIo {
    /// Offers Share, as the web does.
    web: bool,
    exported: RefCell<Vec<(String, String)>>,
    shared: RefCell<Vec<String>>,
    imports_asked: RefCell<u32>,
    inbox: RefCell<VecDeque<IoEvent>>,
}

impl FakeIo {
    fn web() -> Self {
        Self {
            web: true,
            ..Self::default()
        }
    }
}

impl PresetIo for FakeIo {
    fn export(&self, file_name: &str, text: &str) {
        self.exported
            .borrow_mut()
            .push((file_name.to_owned(), text.to_owned()));
        self.inbox
            .borrow_mut()
            .push_back(IoEvent::Exported(file_name.to_owned()));
    }
    fn import(&self) {
        *self.imports_asked.borrow_mut() += 1;
    }
    fn import_dropped(&self, handles: &[u32]) -> bool {
        !handles.is_empty()
    }
    fn can_share(&self) -> bool {
        self.web
    }
    fn share(&self, text: &str) -> Result<(), String> {
        self.shared.borrow_mut().push(text.to_owned());
        Ok(())
    }
    fn take_event(&self) -> Option<IoEvent> {
        self.inbox.borrow_mut().pop_front()
    }
}

fn harness(io: &Rc<FakeIo>, scale: f32) -> Harness {
    harness_with(io, MemoryStore::default(), scale)
}

fn harness_with(io: &Rc<FakeIo>, store: MemoryStore, scale: f32) -> Harness {
    let io: Rc<dyn PresetIo> = io.clone();
    Harness::new(
        move |link| {
            let mut e = widgets::editor_with(link, Box::new(store));
            e.state.set_preset_io(io);
            e
        },
        scale,
    )
}

/// A browser like the one the field opens, with the I/O buttons.
fn geometry(h: &Harness) -> PresetBrowser {
    PresetBrowser::new(
        PresetBrowser::centred(widgets::faceplate::LAYOUT.size),
        Box::new(MemoryStore::default()),
        widgets::FACTORY,
        h.editor().state(),
    )
}

fn button(b: &PresetBrowser, label: &str) -> Point {
    let i = b
        .footer_labels()
        .iter()
        .position(|l| *l == label)
        .unwrap_or_else(|| panic!("no {label} in {:?}", b.footer_labels()));
    b.footer_button(i).center()
}

/// Row `i` of Export's menu.
fn export_item(b: &PresetBrowser, i: usize) -> Point {
    let r = b.export_menu_rect();
    Point::new(r.x + 20.0, r.y + 3.0 + ROW_H * (i as f32 + 0.5))
}

#[test]
fn the_footer_offers_export_import_and_share_with_io() {
    let io = Rc::new(FakeIo::web());
    let mut h = harness(&io, 1.0);
    let b = geometry(&h);
    assert_eq!(
        b.footer_labels(),
        [
            "Export",
            "Import",
            "Share",
            "Random",
            "Init",
            "Save",
            "Save As…"
        ]
    );
    h.click(ids::PROGRAM);
    h.click_at(button(&b, "Export"));
    assert!(io.exported.borrow().is_empty(), "a menu first");
    h.click_at(export_item(&b, 0));
    let exported = io.exported.borrow().clone();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].0, "Init.preset", "named for the current preset");
    assert!(exported[0].1.contains("p2 = 0.5"), "{}", exported[0].1);
    h.advance(16.0);
    assert_eq!(
        h.editor().state().preset_flash(),
        Some("Exported \u{201c}Init.preset\u{201d}")
    );

    h.click_at(button(&b, "Share"));
    assert_eq!(io.shared.borrow().len(), 1);
    h.click_at(button(&b, "Import"));
    assert_eq!(*io.imports_asked.borrow(), 1);
}

#[test]
fn share_is_only_where_the_host_offers_it() {
    let io = Rc::new(FakeIo::default());
    let h = harness(&io, 1.0);
    assert_eq!(
        geometry(&h).footer_labels(),
        ["Export", "Import", "Random", "Init", "Save", "Save As…"]
    );
}

fn user_store() -> MemoryStore {
    let mut s = MemoryStore::default();
    let text = |name: &str| format!("name = {name}\np2 = 0.25\n");
    s.write(&PresetPath::root("Mine"), &text("Mine")).unwrap();
    s.write(&PresetPath::new(Some("Leads"), "Saw"), &text("Saw"))
        .unwrap();
    s
}

#[test]
fn all_user_presets_export_as_a_library() {
    let io = Rc::new(FakeIo::default());
    let mut h = harness_with(&io, user_store(), 1.0);
    let b = geometry(&h);
    h.click(ids::PROGRAM);
    h.click_at(button(&b, "Export"));
    h.click_at(export_item(&b, 1));
    let exported = io.exported.borrow().clone();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].0, "User presets.presetbank");
    let text = &exported[0].1;
    assert!(silkscreen::library::is_library(text));
    assert!(text.contains("\n[Mine]\nname = Mine\n"), "{text}");
    assert!(text.contains("\n[Leads/Saw]\nname = Saw\n"), "{text}");
}

#[test]
fn with_no_user_presets_there_is_nothing_to_export() {
    let io = Rc::new(FakeIo::default());
    let mut h = harness(&io, 1.0);
    let b = geometry(&h);
    h.click(ids::PROGRAM);
    h.click_at(button(&b, "Export"));
    h.click_at(export_item(&b, 1));
    assert!(io.exported.borrow().is_empty());
}

#[test]
fn an_imported_library_merges_and_loads_nothing() {
    let io = Rc::new(FakeIo::default());
    let mut h = harness_with(&io, user_store(), 1.0);
    let library = "# silkscreen preset library\nlibrary = 1\n\
        [Leads/Saw]\nname = Saw\np2 = 0.75\n\
        [Pads/Warm]\nname = Warm\np2 = 0.6\n\
        [Broken]\np2 = loud\n";
    h.click(ids::PROGRAM);
    io.inbox
        .borrow_mut()
        .push_back(IoEvent::Imported(library.into()));
    h.advance(16.0);
    let s = h.editor().state();
    assert_eq!(h.model().get(CUTOFF), 0.5, "nothing loaded");
    let flash = s.preset_flash().unwrap();
    assert!(
        flash.starts_with("Imported 2 presets (1 renamed, 1 skipped: \u{201c}Broken\u{201d}"),
        "{flash}"
    );
    let (text, n) = s.export_library_text().unwrap();
    assert_eq!(n, 4);
    assert!(
        text.contains("[Leads/Saw 2]\nname = Saw\np2 = 0.75"),
        "{text}"
    );
    assert!(text.contains("[Pads/Warm]"), "{text}");
    // The open browser shows the new folder.
    h.advance(16.0);
    assert_snapshot!(h, "browser-library-import");
}

#[test]
fn a_failure_says_why() {
    let io = Rc::new(FakeIo::default());
    let mut h = harness(&io, 1.0);
    io.inbox
        .borrow_mut()
        .push_back(IoEvent::Failed("Couldn't export: disk full".into()));
    h.advance(16.0);
    assert_eq!(
        h.editor().state().preset_flash(),
        Some("Couldn't export: disk full")
    );
}

#[test]
fn without_io_there_are_no_such_buttons() {
    let h = Harness::new(widgets::editor, 1.0);
    let b = geometry(&h);
    assert_eq!(b.footer_labels(), ["Random", "Init", "Save", "Save As…"]);
}

#[test]
fn an_import_arriving_loads_and_says_so() {
    let io = Rc::new(FakeIo::default());
    let mut h = harness(&io, 1.0);
    io.inbox
        .borrow_mut()
        .push_back(IoEvent::Imported("name = From Disk\np2 = 0.1\n".into()));
    h.advance(16.0);
    assert_eq!(h.model().get(CUTOFF), 0.1);
    let s = h.editor().state();
    assert_eq!(s.preset_name(), Some("From Disk"));
    assert_eq!(
        s.preset_place(),
        None,
        "saved nowhere: Save asks for a name"
    );
    assert_eq!(s.preset_flash(), Some("Imported \u{201c}From Disk\u{201d}"));

    io.inbox
        .borrow_mut()
        .push_back(IoEvent::Imported("p2 = loud".into()));
    h.advance(16.0);
    assert!(
        h.editor()
            .state()
            .preset_flash()
            .unwrap()
            .starts_with("Couldn't import")
    );
    assert_eq!(h.model().get(CUTOFF), 0.1, "unchanged");
}

#[test]
fn a_preset_file_dropped_on_the_editor_imports_it() {
    let dir = std::env::temp_dir().join(format!("silkscreen-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Dropped.preset");
    std::fs::write(&file, "name = Dropped\np2 = 0.33\n").unwrap();
    let mut h = Harness::new(widgets::editor, 1.0);
    let anywhere = Point::new(390.0, 10.0);
    h.raw(RawKind::Drop {
        pos: anywhere,
        payload: DropPayload::Paths(vec![file.clone()]),
    });
    assert_eq!(h.model().get(CUTOFF), 0.33);
    assert_eq!(h.editor().state().preset_name(), Some("Dropped"));
    // A library dropped merges.
    let bank = dir.join("Mine.presetbank");
    std::fs::write(
        &bank,
        "# silkscreen preset library\nlibrary = 1\n[Bass/Deep]\nname = Deep\np2 = 0.2\n",
    )
    .unwrap();
    h.raw(RawKind::Drop {
        pos: anywhere,
        payload: DropPayload::Paths(vec![bank]),
    });
    assert_eq!(h.editor().state().preset_flash(), Some("Imported 1 preset"));
    assert_eq!(h.model().get(CUTOFF), 0.33, "a library loads nothing");
    // Other files go to the control under them, as before.
    h.raw(RawKind::Drop {
        pos: anywhere,
        payload: DropPayload::Paths(vec![dir.join("song.wav")]),
    });
    assert_eq!(h.editor().state().preset_name(), Some("Dropped"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn snapshot() {
    let io = Rc::new(FakeIo::web());
    let mut h = harness(&io, 2.0);
    h.click(ids::PROGRAM);
    assert_snapshot!(h, "browser-io@2x");
}

#[test]
fn export_menu_snapshot() {
    let io = Rc::new(FakeIo::web());
    let mut h = harness(&io, 2.0);
    let b = geometry(&h);
    h.click(ids::PROGRAM);
    h.click_at(button(&b, "Export"));
    assert_snapshot!(h, "browser-export-menu@2x");
}
