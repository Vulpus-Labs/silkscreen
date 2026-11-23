//! The preset browser (design/presets.md): a panel centred over
//! the faceplate, with factory categories and user folders on the left
//! and the chosen place's presets on the right.
//!
//! - **Header:** a search field and a close ×. While the field has text,
//!   the right pane lists every place's presets whose names contain it
//!   (ignoring case), each labelled with where it lives.
//! - **Left:** FACTORY and its categories, then USER: Uncategorised, the
//!   folders, and "+ New Folder", which makes a folder and opens a name
//!   field on it.
//! - **Right:** a click (or Enter on the selection) loads a preset, as one
//!   undo step; the current preset is highlighted. The browser opens on
//!   the current preset's place.
//! - **Edits (user presets and folders only):** right-click, or the ⋯ on
//!   a hovered row, opens a menu: Rename, Move to ▸ and Delete for a
//!   preset; Rename and Delete for a folder. Deletes ask first. A preset
//!   dragged onto a user folder moves there, and the browser follows it.
//! - **Footer:** Init, Save and Save As…, and the last outcome or the
//!   store's problem (a failed web write, say). Save overwrites the
//!   current preset if it's the user's, and is Save As otherwise; Save As
//!   asks before replacing. With a host's file services, Export (a menu:
//!   this preset, or every user preset as a library), Import (either
//!   kind) and, on the web, Share come first (design/presets.md).
//!
//! Keys: Up, Down, Page Up, Page Down and Enter work the preset list;
//! everything else types into the search. Esc closes the top panel: a
//! dialog, then a menu, then the browser.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use silkscreen_core::tiny_skia::{PixmapMut, Transform};
use silkscreen_core::{Key, MouseButton, MouseCursor, Point, RawKind, Rect, Size};
use silkscreen_model::{FactoryPreset, Preset};
use silkscreen_text::Style;

use crate::EditorState;
use crate::dialog::{Confirm, SaveAs, centred_over, dots, fill_rounded, outline, outline_in};
use crate::library;
use crate::list::{List, ROW_H};
use crate::panel::{Panel, Reply, Update};
use crate::presets::{PresetIo, PresetPath, PresetStore, UNCATEGORISED, valid_name};
use crate::textfield::TextField;
use crate::theme::{TEXT_SIZE, color, draw_close, theme};
use crate::tooltip::Lettering;

const HEADER_H: f32 = 26.0;
const FOOTER_H: f32 = 28.0;
const GAP: f32 = 4.0;
const FIELD_H: f32 = 18.0;
/// `TextField`'s own inset, for the placeholder.
const FIELD_PAD: f32 = 6.0;
/// The places pane's share of the width, and its bounds.
const LEFT_SHARE: f32 = 0.36;
const LEFT_MIN: f32 = 100.0;
const LEFT_MAX: f32 = 170.0;
/// The browser's preferred size; it shrinks to fit the window.
pub const SIZE: (f32, f32) = (520.0, 320.0);
const INDENT: &str = "   ";
/// The ⋯ zone at a hovered row's right end.
const DOTS_W: f32 = 22.0;
/// How far the pointer travels before a press on a preset becomes a drag.
const DRAG_SLOP: f32 = 4.0;
const MENU_W: f32 = 150.0;
/// The footer's buttons, left to right; with a host's [`PresetIo`],
/// `IO_BUTTONS` come first.
pub const FOOTER_BUTTONS: [&str; 4] = ["Random", "Init", "Save", "Save As…"];
/// A host's Export, Import and (on the web) Share (design/presets.md).
pub const IO_BUTTONS: [&str; 3] = ["Export", "Import", "Share"];
/// Export's menu: the current preset, or every user preset (a library).
pub const EXPORT_MENU: [&str; 2] = ["This preset\u{2026}", "All user presets\u{2026}"];
/// Space either side of a footer button's label.
const BUTTON_PAD: f32 = 8.0;
/// A drag's target folder: the selection red, opaque.
const SELECTED_LINE: [u8; 4] = [226, 38, 46, 255];

/// Where presets live: a factory category or a user folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Factory(&'static str),
    /// `None`: the root, "Uncategorised".
    User(Option<String>),
}

impl Place {
    /// "Factory · Bass", "User · Leads".
    fn label(&self) -> String {
        match self {
            Place::Factory(c) => format!("Factory · {c}"),
            Place::User(f) => format!("User · {}", f.as_deref().unwrap_or(UNCATEGORISED)),
        }
    }
}

/// A row of the places pane.
#[derive(Clone, Debug, PartialEq)]
enum PlaceRow {
    Header(&'static str),
    Place(Place),
    NewFolder,
}

/// A row of the presets pane.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    name: String,
    place: Place,
}

impl Entry {
    /// The store path, for a user preset.
    fn path(&self) -> Option<PresetPath> {
        match &self.place {
            Place::User(folder) => Some(PresetPath::new(folder.as_deref(), &self.name)),
            Place::Factory(_) => None,
        }
    }
}

/// Whether the current preset is `name` in `place`.
fn is_current(state: &EditorState, place: &Place, name: &str) -> bool {
    state.preset_place() == Some(place) && state.preset_name() == Some(name)
}

/// Shared with the menus, fields and dialogs the browser opens.
struct Shared {
    store: Box<dyn PresetStore>,
    factory: &'static [FactoryPreset],
    /// The panes need rebuilding (something they opened committed).
    stale: bool,
    /// The place to show after the rebuild.
    select: Option<Place>,
    /// The last outcome, for the footer.
    message: Option<String>,
}

type SharedRef = Rc<RefCell<Shared>>;

impl Shared {
    /// The factory's categories, in bank order.
    fn categories(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for p in self.factory {
            if !out.contains(&p.category) {
                out.push(p.category);
            }
        }
        out
    }

    /// The user's folders' names.
    fn folders(&self) -> Vec<String> {
        self.store
            .tree()
            .into_iter()
            .filter_map(|f| f.name)
            .collect()
    }

    fn place_rows(&self) -> Vec<PlaceRow> {
        let mut rows = Vec::new();
        let categories = self.categories();
        if !categories.is_empty() {
            rows.push(PlaceRow::Header("FACTORY"));
            rows.extend(
                categories
                    .into_iter()
                    .map(|c| PlaceRow::Place(Place::Factory(c))),
            );
        }
        rows.push(PlaceRow::Header("USER"));
        rows.extend(
            self.store
                .tree()
                .into_iter()
                .map(|f| PlaceRow::Place(Place::User(f.name))),
        );
        rows.push(PlaceRow::NewFolder);
        rows
    }

    /// Every preset, factory first, in order.
    fn all(&self) -> Vec<Entry> {
        let factory = self.factory.iter().map(|p| Entry {
            name: p.name.to_owned(),
            place: Place::Factory(p.category),
        });
        let user = self.store.tree().into_iter().flat_map(|f| {
            let folder = f.name;
            f.presets.into_iter().map(move |name| Entry {
                name,
                place: Place::User(folder.clone()),
            })
        });
        factory.chain(user).collect()
    }

    fn text(&self, entry: &Entry) -> Option<String> {
        match &entry.place {
            Place::Factory(c) => self
                .factory
                .iter()
                .find(|p| p.category == *c && p.name == entry.name)
                .map(|p| p.text.to_owned()),
            Place::User(folder) => self
                .store
                .read(&PresetPath::new(folder.as_deref(), &entry.name)),
        }
    }

    /// Note an outcome and ask for a rebuild showing `select`.
    fn done(&mut self, message: Option<String>, select: Option<Place>) {
        self.message = message;
        if select.is_some() {
            self.select = select;
        }
        self.stale = true;
    }
}

/// The Save As dialog centred over `over`: the current name
/// (or "New Preset") into the current preset's user folder, else a
/// random sound's folder (design/presets.md, offered even before it exists),
/// else `folder`. A taken name asks "Replace?". `None` without presets.
pub(crate) fn save_as_dialog(
    state: &EditorState,
    over: Rect,
    folder: Option<String>,
) -> Option<SaveAs> {
    let (store, _) = state.preset_setup()?;
    let window = state.layout().size;
    let name = state.preset_name().unwrap_or("New Preset").to_owned();
    let mut folders: Vec<String> = store.tree().into_iter().filter_map(|f| f.name).collect();
    let folder = match state.preset_place() {
        Some(Place::User(f)) => f.clone(),
        _ => match state.random_folder() {
            // An existing folder of that name in any case is the one.
            Some(r) => Some(
                if let Some(f) = folders.iter().find(|f| f.eq_ignore_ascii_case(r)) {
                    f.clone()
                } else {
                    folders.push(r.to_owned());
                    crate::presets::sort_names(&mut folders);
                    r.to_owned()
                },
            ),
            None => folder,
        },
    };
    let rect = centred_over(over, SaveAs::SIZE.0, SaveAs::SIZE.1, window);
    let confirm = centred_over(over, Confirm::SIZE.0, Confirm::SIZE.1, window);
    Some(SaveAs::new(
        rect,
        &name,
        folder,
        folders,
        window,
        move |name, folder, state| {
            let name = valid_name(name)?;
            let folder = folder.map(str::to_owned);
            if store.exists(&PresetPath::new(folder.as_deref(), &name)) {
                return Ok(Reply::Replace(Box::new(Confirm::new(
                    confirm,
                    &format!("Replace \u{201c}{name}\u{201d}?"),
                    "Replace",
                    move |state| {
                        match state.write_user_preset(&name, folder.as_deref()) {
                            Ok(()) => state.flash_preset(&format!("Saved \u{201c}{name}\u{201d}")),
                            Err(e) => state.flash_preset(&e),
                        }
                        Reply::Close
                    },
                ))));
            }
            state.write_user_preset(&name, folder.as_deref())?;
            state.flash_preset(&format!("Saved \u{201c}{name}\u{201d}"));
            Ok(Reply::Close)
        },
    ))
}

/// A drag of a user preset: its row, and where the pointer is.
#[derive(Clone, Copy, Debug)]
struct Drag {
    row: usize,
    at: Point,
}

/// The preset browser.
pub struct PresetBrowser {
    rect: Rect,
    window: Size,
    shared: SharedRef,
    search: TextField,
    query: String,
    place: Place,
    place_rows: Vec<PlaceRow>,
    places: List,
    entries: Vec<Entry>,
    presets: List,
    /// Set by the lists' choose callbacks; read after forwarding input.
    chose_place: Rc<Cell<Option<usize>>>,
    chose_preset: Rc<Cell<Option<usize>>>,
    close_hover: bool,
    /// The row under the pointer in each pane, for the ⋯.
    hover_place: Option<usize>,
    hover_preset: Option<usize>,
    hover_button: Option<usize>,
    /// A press on a user preset, which may become a drag.
    press: Option<Drag>,
    drag: Option<Drag>,
    /// The current preset and flash as of the last rebuild, to notice
    /// saves, loads and messages from outside the panes.
    seen: Seen,
    /// The host's Export, Import and Share, if it has them.
    io: Option<Rc<dyn PresetIo>>,
    lettering: Lettering,
}

type Seen = (Option<String>, Option<Place>, Option<String>);

fn seen(state: &EditorState) -> Seen {
    (
        state.preset_name().map(str::to_owned),
        state.preset_place().cloned(),
        state.preset_flash().map(str::to_owned),
    )
}

/// Whether `seen` is still what `state` shows, without copying it.
fn still_seen(seen: &Seen, state: &EditorState) -> bool {
    seen.0.as_deref() == state.preset_name()
        && seen.1.as_ref() == state.preset_place()
        && seen.2.as_deref() == state.preset_flash()
}

impl std::fmt::Debug for PresetBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresetBrowser")
            .field("rect", &self.rect)
            .field("place", &self.place)
            .field("query", &self.query)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl PresetBrowser {
    /// A browser over `rect` of `store`'s presets and the read-only
    /// `factory` bank, open on the current preset's place.
    pub fn new(
        rect: Rect,
        store: Box<dyn PresetStore>,
        factory: &'static [FactoryPreset],
        state: &EditorState,
    ) -> Self {
        let shared = Shared {
            store,
            factory,
            stale: false,
            select: None,
            message: None,
        };
        let place = state
            .preset_place()
            .cloned()
            .or_else(|| {
                let all = shared.all();
                state
                    .preset_name()
                    .and_then(|n| all.into_iter().find(|e| e.name == n))
                    .map(|e| e.place)
            })
            .or_else(|| shared.categories().first().map(|c| Place::Factory(c)))
            .unwrap_or(Place::User(None));
        let placeholder = || {
            List::new(Rect::new(0.0, 0.0, 1.0, 1.0), Vec::new(), |_, _| {
                Reply::Ignored
            })
        };
        let mut b = Self {
            rect,
            window: state.layout().size,
            shared: Rc::new(RefCell::new(shared)),
            search: TextField::new(Rect::new(0.0, 0.0, 1.0, 1.0), "", |_, _| Reply::Ignored),
            query: String::new(),
            place,
            place_rows: Vec::new(),
            places: placeholder(),
            entries: Vec::new(),
            presets: placeholder(),
            chose_place: Rc::new(Cell::new(None)),
            chose_preset: Rc::new(Cell::new(None)),
            close_hover: false,
            hover_place: None,
            hover_preset: None,
            hover_button: None,
            press: None,
            drag: None,
            seen: seen(state),
            io: state.preset_io(),
            lettering: Lettering::shared(),
        };
        b.search = b.new_search();
        b.rebuild_places();
        b.rebuild_presets(state);
        b
    }

    /// The browser's rect over a `window`: `SIZE`, shrunk to fit with an
    /// 8-unit margin, centred.
    pub fn centred(window: Size) -> Rect {
        let w = SIZE.0.min(window.w - 16.0);
        let h = SIZE.1.min(window.h - 16.0);
        Rect::new(
            ((window.w - w) / 2.0).round(),
            ((window.h - h) / 2.0).round(),
            w,
            h,
        )
    }

    /// The place shown.
    pub fn place(&self) -> &Place {
        &self.place
    }

    /// The places pane's labels, as drawn.
    pub fn place_labels(&self) -> Vec<String> {
        self.place_rows.iter().map(place_label).collect()
    }

    /// The presets pane's labels, as drawn.
    pub fn preset_labels(&self) -> Vec<String> {
        self.entries.iter().map(|e| self.entry_label(e)).collect()
    }

    /// The search field's rect.
    pub fn search_rect(&self) -> Rect {
        let r = self.rect;
        Rect::new(
            r.x + GAP + 1.0,
            r.y + 1.0 + (HEADER_H - FIELD_H) / 2.0,
            r.w - 2.0 * GAP - 2.0 - HEADER_H,
            FIELD_H,
        )
    }

    /// The close button's rect.
    pub fn close_rect(&self) -> Rect {
        let r = self.rect;
        Rect::new(r.right() - 1.0 - HEADER_H, r.y + 1.0, HEADER_H, HEADER_H)
    }

    fn body(&self) -> Rect {
        let r = self.rect;
        Rect::new(
            r.x + 1.0,
            r.y + 1.0 + HEADER_H,
            r.w - 2.0,
            r.h - 2.0 - HEADER_H - FOOTER_H,
        )
    }

    /// The places pane's rect.
    pub fn places_rect(&self) -> Rect {
        let b = self.body();
        let w = (self.rect.w * LEFT_SHARE).clamp(LEFT_MIN, LEFT_MAX);
        Rect::new(b.x + GAP - 1.0, b.y, w, b.h)
    }

    /// The presets pane's rect.
    pub fn presets_rect(&self) -> Rect {
        let b = self.body();
        let left = self.places_rect();
        let x = left.right() + GAP;
        Rect::new(x, b.y, b.right() - GAP + 1.0 - x, b.h)
    }

    /// The footer's buttons, left to right: `IO_BUTTONS` if the host
    /// has them (Share only if it offers it), then `FOOTER_BUTTONS`.
    pub fn footer_labels(&self) -> Vec<&'static str> {
        let io: &[&str] = match &self.io {
            Some(io) if io.can_share() => &IO_BUTTONS,
            Some(_) => &IO_BUTTONS[..2],
            None => &[],
        };
        io.iter().chain(&FOOTER_BUTTONS).copied().collect()
    }

    /// Footer button `index`'s rect (of [`footer_labels`](Self::footer_labels)),
    /// right-aligned, each as wide as its label, shrinking together if
    /// they don't fit.
    pub fn footer_button(&self, index: usize) -> Rect {
        let labels = self.footer_labels();
        let mut text = self.lettering.text.borrow_mut();
        let widths: Vec<f32> = labels
            .iter()
            .map(|l| text.width(&self.lettering.face, l, TEXT_SIZE, 1.0) + 2.0 * BUTTON_PAD)
            .collect();
        drop(text);
        let room = self.rect.w - 2.0 - GAP;
        let total: f32 = widths.iter().map(|w| w + GAP).sum();
        let shrink = (room / total).min(1.0);
        let after: f32 = widths[index..].iter().map(|w| (w + GAP) * shrink).sum();
        let x = (self.rect.right() - 1.0 - after).round();
        let height = FOOTER_H - 2.0 * GAP;
        let y = self.rect.bottom() - 1.0 - GAP - height;
        Rect::new(x, y, (widths[index] * shrink).floor(), height)
    }

    /// The footer button under `pos`, by label.
    fn footer_at(&self, pos: Point) -> Option<&'static str> {
        let labels = self.footer_labels();
        (0..labels.len())
            .find(|&i| self.footer_button(i).contains(pos))
            .map(|i| labels[i])
    }

    /// Row `i`'s rect in a pane at `pane`, scrolled by `scroll`.
    fn row_rect(pane: Rect, i: usize, scroll: f32) -> Rect {
        Rect::new(
            pane.x + 1.0,
            pane.y + 3.0 + i as f32 * ROW_H - scroll,
            pane.w - 2.0,
            ROW_H,
        )
    }

    /// The row of `n` under `pos` in a pane at `pane`, scrolled by
    /// `scroll`.
    fn row_at(pane: Rect, scroll: f32, n: usize, pos: Point) -> Option<usize> {
        let inner = Rect::new(pane.x + 1.0, pane.y + 3.0, pane.w - 2.0, pane.h - 6.0);
        if !inner.contains(pos) {
            return None;
        }
        let i = ((pos.y - inner.y + scroll) / ROW_H).floor();
        (i >= 0.0 && (i as usize) < n).then_some(i as usize)
    }

    /// Preset row `i`'s rect.
    pub fn preset_row(&self, i: usize) -> Rect {
        Self::row_rect(self.presets_rect(), i, self.presets.scroll())
    }

    /// Place row `i`'s rect.
    pub fn place_row(&self, i: usize) -> Rect {
        Self::row_rect(self.places_rect(), i, self.places.scroll())
    }

    /// The ⋯ zone of a row.
    pub fn dots_rect(row: Rect) -> Rect {
        Rect::new(row.right() - DOTS_W - 8.0, row.y, DOTS_W, row.h)
    }

    fn footer(&self) -> Rect {
        let r = self.rect;
        Rect::new(r.x + 1.0, r.bottom() - 1.0 - FOOTER_H, r.w - 2.0, FOOTER_H)
    }

    /// An empty search field.
    fn new_search(&self) -> TextField {
        TextField::new(self.search_rect(), "", |_, _| Reply::Ignored)
    }

    fn entry_label(&self, e: &Entry) -> String {
        if self.query.is_empty() {
            e.name.clone()
        } else {
            format!("{}  ·  {}", e.name, e.place.label())
        }
    }

    fn rebuild_places(&mut self) {
        self.place_rows = self.shared.borrow().place_rows();
        let labels = self.place_labels();
        let dimmed = self
            .place_rows
            .iter()
            .map(|r| matches!(r, PlaceRow::Header(_)))
            .collect();
        let chose = Rc::clone(&self.chose_place);
        let mut list = List::new(self.places_rect(), labels, move |i, _| {
            chose.set(Some(i));
            Reply::Handled
        })
        .with_dimmed(dimmed);
        if let Some(i) = self
            .place_rows
            .iter()
            .position(|r| *r == PlaceRow::Place(self.place.clone()))
        {
            list = list.with_selected(i);
        }
        self.places = list;
    }

    /// Rebuild the presets pane, highlighting the current preset if it's
    /// there.
    fn rebuild_presets(&mut self, state: &EditorState) {
        let all = self.shared.borrow().all();
        let needle = self.query.trim().to_lowercase();
        self.entries = if needle.is_empty() {
            all.into_iter().filter(|e| e.place == self.place).collect()
        } else {
            all.into_iter()
                .filter(|e| e.name.to_lowercase().contains(&needle))
                .collect()
        };
        let labels = self.preset_labels();
        let chose = Rc::clone(&self.chose_preset);
        let mut list = List::new(self.presets_rect(), labels, move |i, _| {
            chose.set(Some(i));
            Reply::Handled
        });
        let current = self.entries.iter().position(|e| {
            is_current(state, &e.place, &e.name)
                || (state.preset_place().is_none() && state.preset_name() == Some(&e.name))
        });
        if let Some(i) = current {
            list = list.with_selected(i);
        }
        self.presets = list;
    }

    fn rebuild(&mut self, state: &EditorState) {
        self.rebuild_places();
        self.rebuild_presets(state);
    }

    fn say(&self, message: Option<String>) {
        self.shared.borrow_mut().message = message;
    }

    /// Act on a place row chosen in the left pane.
    fn choose_place(&mut self, i: usize, state: &EditorState) -> Reply {
        match self.place_rows.get(i).cloned() {
            Some(PlaceRow::Place(place)) => {
                self.place = place;
                self.query.clear();
                self.search = self.new_search();
                self.rebuild(state);
                Reply::Handled
            }
            Some(PlaceRow::NewFolder) => self.new_folder(state),
            // Headers aren't places: put the selection back.
            Some(PlaceRow::Header(_)) | None => {
                self.rebuild_places();
                Reply::Handled
            }
        }
    }

    /// Make a folder, show it, and open a name field over its row.
    fn new_folder(&mut self, state: &EditorState) -> Reply {
        let made = self.shared.borrow_mut().store.create_folder();
        let name = match made {
            Ok(name) => name,
            Err(e) => {
                self.say(Some(e));
                self.rebuild_places();
                return Reply::Handled;
            }
        };
        self.say(None);
        self.place = Place::User(Some(name.clone()));
        self.query.clear();
        self.search = self.new_search();
        self.rebuild(state);
        let i = self
            .place_rows
            .iter()
            .position(|r| *r == PlaceRow::Place(self.place.clone()))
            .unwrap_or(0);
        Reply::Open(Box::new(folder_rename(
            &self.shared,
            self.place_row(i),
            &name,
            true,
        )))
    }

    /// Load the preset in row `i` of the right pane.
    fn load(&mut self, i: usize, state: &mut EditorState) -> Reply {
        let Some(entry) = self.entries.get(i).cloned() else {
            return Reply::Ignored;
        };
        let text = self.shared.borrow().text(&entry);
        let parsed = text
            .ok_or_else(|| format!("\u{201c}{}\u{201d} has gone", entry.name))
            .and_then(|t| {
                Preset::parse(&t).map_err(|e| format!("\u{201c}{}\u{201d}: {e}", entry.name))
            });
        match parsed {
            Ok(mut preset) => {
                preset.name.clone_from(&entry.name);
                state.load_preset(&preset);
                state.set_preset_place(Some(entry.place));
                self.say(None);
            }
            Err(e) => self.say(Some(e)),
        }
        Reply::Handled
    }

    /// A footer button, by label.
    fn press_footer(&mut self, label: &str, state: &mut EditorState) -> Reply {
        match label {
            "Init" => self.init(state),
            "Random" => {
                state.randomise_preset();
                self.say(None);
                Reply::Handled
            }
            "Save" => self.save(state),
            "Save As…" => self.save_as(state),
            _ => {
                let Some(io) = self.io.clone() else {
                    return Reply::Ignored;
                };
                match label {
                    "Export" => return Reply::Open(Box::new(self.export_menu(io))),
                    "Import" => io.import(),
                    _ => match io.share(&state.current_preset_text().1) {
                        Ok(()) => self.say(Some("Link copied".to_owned())),
                        Err(e) => self.say(Some(e)),
                    },
                }
                Reply::Handled
            }
        }
    }

    /// Where Export's menu opens: just above its button.
    pub fn export_menu_rect(&self) -> Rect {
        let button = self
            .footer_labels()
            .iter()
            .position(|l| *l == "Export")
            .map_or(self.footer(), |i| self.footer_button(i));
        let h = EXPORT_MENU.len() as f32 * ROW_H + 6.0;
        menu_rect(
            Point::new(button.x, button.y - h),
            EXPORT_MENU.len(),
            self.window,
        )
    }

    /// Export's menu, just above its button (design/presets.md): the current
    /// preset as `<name>.preset`, or every user preset as a library.
    /// How the export went arrives later, as the host's event.
    fn export_menu(&self, io: Rc<dyn PresetIo>) -> List {
        let rect = self.export_menu_rect();
        let shared = Rc::clone(&self.shared);
        List::new(
            rect,
            EXPORT_MENU.iter().map(|s| (*s).to_owned()).collect(),
            move |choice, state| {
                if choice == 0 {
                    let (name, text) = state.current_preset_text();
                    io.export(&format!("{name}.{}", crate::presets::EXT), &text);
                } else {
                    match state.export_library_text() {
                        Some((text, n)) if n > 0 => io.export(library::FILE_NAME, &text),
                        _ => shared
                            .borrow_mut()
                            .done(Some("No user presets to export".to_owned()), None),
                    }
                }
                Reply::Close
            },
        )
    }

    /// Footer: Save over the current user preset, else Save As.
    fn save(&mut self, state: &mut EditorState) -> Reply {
        match state.save_preset_in_place() {
            Some(Ok(())) => {
                let name = state.preset_name().unwrap_or_default().to_owned();
                state.flash_preset(&format!("Saved \u{201c}{name}\u{201d}"));
                Reply::Handled
            }
            Some(Err(e)) => {
                self.say(Some(e));
                Reply::Handled
            }
            None => self.save_as(state),
        }
    }

    /// Footer: the Save As dialog, on the current (else shown) user folder.
    fn save_as(&mut self, state: &EditorState) -> Reply {
        let shown = match &self.place {
            Place::User(f) => f.clone(),
            Place::Factory(_) => None,
        };
        match save_as_dialog(state, self.rect, shown) {
            Some(dialog) => Reply::Open(Box::new(dialog)),
            None => Reply::Ignored,
        }
    }

    /// Footer: every param to its default, as one undo step.
    fn init(&mut self, state: &mut EditorState) -> Reply {
        state.init_preset();
        self.say(None);
        Reply::Handled
    }

    /// The menu for user preset row `i`, at `at`.
    fn preset_menu(&self, i: usize, at: Point) -> Option<List> {
        let entry = self.entries.get(i)?;
        let path = entry.path()?;
        let row = self.preset_row(i);
        let shared = Rc::clone(&self.shared);
        let window = self.window;
        let confirm = centred_over(self.rect, Confirm::SIZE.0, Confirm::SIZE.1, window);
        let place = entry.place.clone();
        let items = ["Rename", "Move to\u{2026}", "Delete"];
        let rect = menu_rect(at, items.len(), window);
        Some(List::new(
            rect,
            items.iter().map(|s| (*s).to_owned()).collect(),
            move |choice, _| match choice {
                0 => Reply::Replace(Box::new(preset_rename(&shared, row, &path, &place))),
                1 => Reply::Replace(Box::new(move_menu(&shared, rect, &path, &place, window))),
                _ => Reply::Replace(Box::new(preset_delete(&shared, confirm, &path, &place))),
            },
        ))
    }

    /// The menu for place row `i` if it's a named user folder, at `at`.
    fn folder_menu(&self, i: usize, at: Point) -> Option<List> {
        let Some(PlaceRow::Place(Place::User(Some(name)))) = self.place_rows.get(i) else {
            return None;
        };
        let name = name.clone();
        let row = self.place_row(i);
        let shared = Rc::clone(&self.shared);
        let shown = self.place == Place::User(Some(name.clone()));
        let confirm = centred_over(self.rect, Confirm::SIZE.0, Confirm::SIZE.1, self.window);
        let items = ["Rename", "Delete"];
        Some(List::new(
            menu_rect(at, items.len(), self.window),
            items.iter().map(|s| (*s).to_owned()).collect(),
            move |choice, _| match choice {
                0 => Reply::Replace(Box::new(folder_rename(&shared, row, &name, shown))),
                _ => Reply::Replace(Box::new(folder_delete(&shared, confirm, &name))),
            },
        ))
    }

    /// A menu for whatever is under `pos`, if it has one.
    fn menu_at(&self, pos: Point) -> Option<List> {
        let presets = self.presets_rect();
        if let Some(i) = Self::row_at(presets, self.presets.scroll(), self.entries.len(), pos) {
            return self.preset_menu(i, pos);
        }
        let places = self.places_rect();
        Self::row_at(places, self.places.scroll(), self.place_rows.len(), pos)
            .and_then(|i| self.folder_menu(i, pos))
    }

    /// Whether `pos` is on the ⋯ of a row that has a menu.
    fn on_dots(&self, pos: Point) -> bool {
        let presets = self.presets_rect();
        if let Some(i) = Self::row_at(presets, self.presets.scroll(), self.entries.len(), pos) {
            return self.entries[i].path().is_some()
                && Self::dots_rect(self.preset_row(i)).contains(pos);
        }
        let places = self.places_rect();
        Self::row_at(places, self.places.scroll(), self.place_rows.len(), pos).is_some_and(|i| {
            matches!(self.place_rows[i], PlaceRow::Place(Place::User(Some(_))))
                && Self::dots_rect(self.place_row(i)).contains(pos)
        })
    }

    /// The user folder a drag would drop into at `pos`, if it can.
    fn drop_target(&self, drag: Drag) -> Option<(usize, Option<String>)> {
        let entry = self.entries.get(drag.row)?;
        let places = self.places_rect();
        let i = Self::row_at(places, self.places.scroll(), self.place_rows.len(), drag.at)?;
        match &self.place_rows[i] {
            PlaceRow::Place(Place::User(folder)) if entry.place != Place::User(folder.clone()) => {
                Some((i, folder.clone()))
            }
            _ => None,
        }
    }

    /// Finish a drag: move the preset if it's over another user folder.
    fn drop_preset(&mut self, drag: Drag, state: &mut EditorState) -> Reply {
        let Some((_, folder)) = self.drop_target(drag) else {
            return Reply::Handled;
        };
        let entry = self.entries[drag.row].clone();
        let Some(path) = entry.path() else {
            return Reply::Handled;
        };
        let moved = self
            .shared
            .borrow_mut()
            .store
            .move_to(&path, folder.as_deref());
        match moved {
            Ok(()) => {
                let to = Place::User(folder);
                if is_current(state, &entry.place, &entry.name) {
                    state.set_preset_place(Some(to.clone()));
                }
                self.place = to;
                self.say(None);
            }
            Err(e) => self.say(Some(e)),
        }
        self.rebuild(state);
        Reply::Handled
    }

    /// Forward `event` to the pane under its position, or to both panes
    /// for releases (so drags in either finish).
    fn forward(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        let pos = pointer(event);
        let release = matches!(event, RawKind::PointerUp { .. });
        let to_places = release || pos.is_none_or(|p| self.places_rect().contains(p));
        let to_presets = release || pos.is_none_or(|p| self.presets_rect().contains(p));
        let mut reply = Reply::Ignored;
        if to_places || matches!(event, RawKind::PointerMove { .. }) {
            reply = reply.and(self.places.input(event, state));
        }
        if to_presets || matches!(event, RawKind::PointerMove { .. }) {
            reply = reply.and(self.presets.input(event, state));
        }
        if let Some(i) = self.chose_place.take() {
            return self.choose_place(i, state);
        }
        if let Some(i) = self.chose_preset.take() {
            return self.load(i, state);
        }
        reply
    }

    fn pointer_down(
        &mut self,
        pos: Point,
        button: MouseButton,
        event: &RawKind,
        state: &mut EditorState,
    ) -> Reply {
        if self.close_rect().contains(pos) {
            return Reply::Close;
        }
        if self.search_rect().contains(pos) {
            return self.search.input(event, state);
        }
        if let Some(label) = self.footer_at(pos) {
            return self.press_footer(label, state);
        }
        if button == MouseButton::Right || self.on_dots(pos) {
            return match self.menu_at(pos) {
                Some(menu) => Reply::Open(Box::new(menu)),
                None => Reply::Handled,
            };
        }
        let presets = self.presets_rect();
        self.press = Self::row_at(presets, self.presets.scroll(), self.entries.len(), pos)
            .filter(|&i| self.entries[i].path().is_some())
            .map(|row| Drag { row, at: pos });
        self.forward(event, state)
    }

    /// Where a drag's ghost (the preset's name at the pointer) draws, at
    /// `scale` physical pixels a unit.
    fn ghost_rect(&self, drag: Drag, scale: f32) -> Option<Rect> {
        let entry = self.entries.get(drag.row)?;
        let w = self.lettering.text.borrow_mut().width(
            &self.lettering.face,
            &entry.name,
            TEXT_SIZE,
            scale,
        );
        let ghost = Rect::new(drag.at.x + 10.0, drag.at.y - ROW_H / 2.0, w + 12.0, ROW_H);
        Some(Rect::new(
            ghost.x.min(self.rect.right() - 1.0 - ghost.w),
            ghost
                .y
                .clamp(self.rect.y + 1.0, self.rect.bottom() - 1.0 - ghost.h),
            ghost.w,
            ghost.h,
        ))
    }

    /// What a drag draws: its ghost and its target folder's outline,
    /// padded for text measured at another scale.
    fn drag_rects(&self, drag: Drag) -> Vec<Rect> {
        let pad = |r: Rect| Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0);
        let target = self.drop_target(drag).map(|(i, _)| self.place_row(i));
        self.ghost_rect(drag, 1.0)
            .into_iter()
            .chain(target)
            .map(pad)
            .collect()
    }

    fn pointer_move(&mut self, pos: Point, event: &RawKind, state: &mut EditorState) -> Reply {
        if let Some(press) = self.press {
            let moved = (pos.x - press.at.x).hypot(pos.y - press.at.y);
            if self.drag.is_some() || moved > DRAG_SLOP {
                // Repaint where the drag drew and where it draws now; as
                // it starts, the hovered rows too, whose ⋯ a drag hides.
                let mut rects = if let Some(d) = self.drag {
                    self.drag_rects(d)
                } else {
                    let (places, presets) = (self.places_rect(), self.presets_rect());
                    let inner = |p: Rect| Rect::new(p.x + 1.0, p.y + 3.0, p.w - 2.0, p.h - 6.0);
                    let place = self.hover_place.and_then(|i| {
                        Self::row_rect(places, i, self.places.scroll()).intersect(&inner(places))
                    });
                    let preset = self.hover_preset.and_then(|i| {
                        Self::row_rect(presets, i, self.presets.scroll()).intersect(&inner(presets))
                    });
                    place.into_iter().chain(preset).collect()
                };
                let drag = Drag {
                    row: press.row,
                    at: pos,
                };
                self.drag = Some(drag);
                rects.extend(self.drag_rects(drag));
                return Reply::repaint(rects);
            }
        }
        let close_hover = self.close_rect().contains(pos);
        let hover_place = Self::row_at(
            self.places_rect(),
            self.places.scroll(),
            self.place_rows.len(),
            pos,
        );
        let hover_preset = Self::row_at(
            self.presets_rect(),
            self.presets.scroll(),
            self.entries.len(),
            pos,
        );
        let hover_button =
            (0..self.footer_labels().len()).find(|&i| self.footer_button(i).contains(pos));
        // Repaint what the hover left and entered: the X, rows (for
        // their ⋯) and footer buttons.
        let mut rects = Vec::new();
        if close_hover != self.close_hover {
            rects.push(self.close_rect());
        }
        let rows = |pane: Rect, scroll: f32, was: Option<usize>, now: Option<usize>| {
            let inner = Rect::new(pane.x + 1.0, pane.y + 3.0, pane.w - 2.0, pane.h - 6.0);
            [was, now]
                .into_iter()
                .flatten()
                .filter_map(move |i| Self::row_rect(pane, i, scroll).intersect(&inner))
        };
        if hover_place != self.hover_place {
            let pane = self.places_rect();
            rects.extend(rows(
                pane,
                self.places.scroll(),
                self.hover_place,
                hover_place,
            ));
        }
        if hover_preset != self.hover_preset {
            let pane = self.presets_rect();
            rects.extend(rows(
                pane,
                self.presets.scroll(),
                self.hover_preset,
                hover_preset,
            ));
        }
        if hover_button != self.hover_button {
            for i in [self.hover_button, hover_button].into_iter().flatten() {
                rects.push(self.footer_button(i));
            }
        }
        self.close_hover = close_hover;
        self.hover_place = hover_place;
        self.hover_preset = hover_preset;
        self.hover_button = hover_button;
        let search = self.search.input(event, state);
        let panes = self.forward(event, state);
        Reply::repaint(rects).and(search).and(panes)
    }

    fn draw_dots(canvas: &mut PixmapMut<'_>, row: Rect, transform: Transform) {
        let r = Self::dots_rect(row);
        dots(canvas, r.center().x, r.center().y, theme().ink, transform);
    }

    fn draw_text(
        &self,
        canvas: &mut PixmapMut<'_>,
        text: &str,
        x: f32,
        r: Rect,
        ink: [u8; 4],
        transform: Transform,
    ) {
        let face = &self.lettering.face;
        let (ascent, descent) = face.line_metrics(TEXT_SIZE);
        let style = Style {
            face,
            size: TEXT_SIZE,
            color: color(ink),
        };
        self.lettering.text.borrow_mut().draw_clipped(
            canvas,
            text,
            (x, r.center().y + f32::midpoint(ascent, descent)),
            &style,
            transform,
            Some([r.x, r.y, r.right(), r.bottom()]),
        );
    }
}

/// A menu of `n` rows at `at`, kept inside `window`.
fn menu_rect(at: Point, n: usize, window: Size) -> Rect {
    let h = n as f32 * ROW_H + 6.0;
    let x = at.x.min(window.w - 2.0 - MENU_W).max(2.0);
    let y = if at.y + h <= window.h - 2.0 {
        at.y
    } else {
        (at.y - h).max(2.0)
    };
    Rect::new(x, y, MENU_W, h)
}

/// A name field over preset `path`'s `row`, renaming it on Enter.
fn preset_rename(shared: &SharedRef, row: Rect, path: &PresetPath, place: &Place) -> TextField {
    let shared = Rc::clone(shared);
    let path = path.clone();
    let place = place.clone();
    TextField::new(row, &path.name.clone(), move |text, state| {
        let mut s = shared.borrow_mut();
        match s.store.rename(&path, text) {
            Ok(()) => {
                if is_current(state, &place, &path.name) {
                    state.set_preset_name(Some(text.trim().to_owned()));
                }
                s.done(None, None);
                Reply::Close
            }
            Err(e) => {
                s.done(Some(e), None);
                Reply::Handled
            }
        }
    })
    .with_title("Preset Name")
}

/// The folders preset `path` can move to, as a menu where its menu was.
fn move_menu(shared: &SharedRef, at: Rect, path: &PresetPath, place: &Place, window: Size) -> List {
    let mut dests: Vec<Option<String>> = vec![None];
    dests.extend(shared.borrow().folders().into_iter().map(Some));
    dests.retain(|d| *d != path.folder);
    let labels = dests
        .iter()
        .map(|d| d.clone().unwrap_or_else(|| UNCATEGORISED.to_owned()))
        .collect();
    let rect = menu_rect(Point::new(at.x, at.y), dests.len().max(1), window);
    let shared = Rc::clone(shared);
    let path = path.clone();
    let place = place.clone();
    List::new(rect, labels, move |i, state| {
        let dest = dests[i].clone();
        let mut s = shared.borrow_mut();
        match s.store.move_to(&path, dest.as_deref()) {
            Ok(()) => {
                let to = Place::User(dest);
                if is_current(state, &place, &path.name) {
                    state.set_preset_place(Some(to.clone()));
                }
                s.done(None, Some(to));
            }
            Err(e) => s.done(Some(e), None),
        }
        Reply::Close
    })
}

/// "Delete “X”?" for preset `path`.
fn preset_delete(shared: &SharedRef, rect: Rect, path: &PresetPath, place: &Place) -> Confirm {
    let shared = Rc::clone(shared);
    let path = path.clone();
    let place = place.clone();
    Confirm::new(
        rect,
        &format!("Delete \u{201c}{}\u{201d}?", path.name),
        "Delete",
        move |state| {
            let mut s = shared.borrow_mut();
            match s.store.remove(&path) {
                Ok(()) => {
                    // The sound stays; it's no longer saved anywhere.
                    if is_current(state, &place, &path.name) {
                        state.set_preset_place(None);
                    }
                    s.done(Some(format!("Deleted \u{201c}{}\u{201d}", path.name)), None);
                }
                Err(e) => s.done(Some(e), None),
            }
            Reply::Close
        },
    )
}

/// A name field over folder `name`'s `row`, renaming it on Enter; the
/// browser follows it if it was `shown`. A taken name keeps the field
/// open.
fn folder_rename(shared: &SharedRef, row: Rect, name: &str, shown: bool) -> TextField {
    let shared = Rc::clone(shared);
    let from = name.to_owned();
    TextField::new(row, name, move |text, state| {
        let mut s = shared.borrow_mut();
        match s.store.rename_folder(&from, text) {
            Ok(()) => {
                let to = Place::User(Some(text.trim().to_owned()));
                if state.preset_place() == Some(&Place::User(Some(from.clone()))) {
                    state.set_preset_place(Some(to.clone()));
                }
                s.done(None, shown.then_some(to));
                Reply::Close
            }
            Err(e) => {
                s.done(Some(e), None);
                Reply::Handled
            }
        }
    })
    .with_title("Folder Name")
}

/// "Delete “F” and its N presets?" for folder `name`.
fn folder_delete(shared: &SharedRef, rect: Rect, name: &str) -> Confirm {
    let count = shared
        .borrow()
        .store
        .tree()
        .into_iter()
        .find(|f| f.name.as_deref() == Some(name))
        .map_or(0, |f| f.presets.len());
    let message = match count {
        0 => format!("Delete \u{201c}{name}\u{201d}?"),
        1 => format!("Delete \u{201c}{name}\u{201d} and its preset?"),
        n => format!("Delete \u{201c}{name}\u{201d} and its {n} presets?"),
    };
    let shared = Rc::clone(shared);
    let name = name.to_owned();
    Confirm::new(rect, &message, "Delete", move |state| {
        let mut s = shared.borrow_mut();
        match s.store.remove_folder(&name) {
            Ok(()) => {
                if state.preset_place() == Some(&Place::User(Some(name.clone()))) {
                    state.set_preset_place(None);
                }
                s.done(Some(format!("Deleted \u{201c}{name}\u{201d}")), None);
            }
            Err(e) => s.done(Some(e), None),
        }
        Reply::Close
    })
}

fn place_label(row: &PlaceRow) -> String {
    match row {
        PlaceRow::Header(h) => (*h).to_owned(),
        PlaceRow::Place(Place::Factory(c)) => format!("{INDENT}{c}"),
        PlaceRow::Place(Place::User(f)) => {
            format!("{INDENT}{}", f.as_deref().unwrap_or(UNCATEGORISED))
        }
        PlaceRow::NewFolder => format!("{INDENT}+ New Folder"),
    }
}

/// The pointer position an event carries, if it's a pointer event.
fn pointer(event: &RawKind) -> Option<Point> {
    match *event {
        RawKind::PointerDown { pos, .. }
        | RawKind::PointerUp { pos, .. }
        | RawKind::PointerMove { pos, .. }
        | RawKind::Wheel { pos, .. } => Some(pos),
        _ => None,
    }
}

impl Panel for PresetBrowser {
    fn rect(&self) -> Rect {
        self.rect
    }

    /// The search field takes what's typed while the browser is open.
    fn takes_text(&self) -> bool {
        true
    }

    fn cursor(&self, pos: Point) -> MouseCursor {
        let preset = Self::row_at(
            self.presets_rect(),
            self.presets.scroll(),
            self.entries.len(),
            pos,
        );
        let place = Self::row_at(
            self.places_rect(),
            self.places.scroll(),
            self.place_rows.len(),
            pos,
        );
        if preset.is_some() || place.is_some() {
            MouseCursor::Hand
        } else {
            MouseCursor::Default
        }
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        fill_rounded(canvas, self.rect, theme().background, transform);

        // Header: the search field (with a placeholder while empty) and ×.
        self.search.draw(canvas, transform);
        if self.search.text().is_empty() {
            let r = self.search_rect();
            self.draw_text(
                canvas,
                "Search",
                r.x + FIELD_PAD + 2.0,
                r,
                theme().dim_ink,
                transform,
            );
        }
        let close = self.close_rect();
        let glyph = Rect::new(close.x + 4.0, close.y + 4.0, close.w - 8.0, close.h - 8.0);
        draw_close(canvas, glyph, self.close_hover, transform);

        self.places.draw(canvas, transform);
        self.presets.draw(canvas, transform);

        // The ⋯ on a hovered row that has a menu (not while dragging).
        if self.drag.is_none() {
            if let Some(i) = self.hover_preset
                && self.entries.get(i).is_some_and(|e| e.path().is_some())
            {
                Self::draw_dots(canvas, self.preset_row(i), transform);
            }
            if let Some(i) = self.hover_place
                && matches!(
                    self.place_rows.get(i),
                    Some(PlaceRow::Place(Place::User(Some(_))))
                )
            {
                Self::draw_dots(canvas, self.place_row(i), transform);
            }
        }

        // Footer: the outcome (or the store's problem), then the buttons.
        let (message, problem) = {
            let s = self.shared.borrow();
            (s.message.clone(), s.store.problem())
        };
        let f = self.footer();
        let first = self.footer_button(0);
        if let Some(line) = message.or(problem) {
            let r = Rect::new(f.x, f.y, first.x - GAP - f.x, f.h);
            self.draw_text(canvas, &line, r.x + GAP + 2.0, r, theme().ink, transform);
        }
        for (i, label) in self.footer_labels().iter().enumerate() {
            let r = self.footer_button(i);
            if self.hover_button == Some(i) {
                fill_rounded(canvas, r, theme().hover, transform);
            }
            outline(canvas, r, transform);
            let w = self.lettering.text.borrow_mut().width(
                &self.lettering.face,
                label,
                TEXT_SIZE,
                transform.sx,
            );
            self.draw_text(
                canvas,
                label,
                r.center().x - w / 2.0,
                r,
                theme().ink,
                transform,
            );
        }

        // A drag: the target folder lit, and the preset's name at the
        // pointer.
        if let Some(drag) = self.drag {
            if let Some((i, _)) = self.drop_target(drag) {
                outline_in(canvas, self.place_row(i), SELECTED_LINE, 2.0, transform);
            }
            if let (Some(entry), Some(ghost)) = (
                self.entries.get(drag.row),
                self.ghost_rect(drag, transform.sx),
            ) {
                fill_rounded(canvas, ghost, theme().background, transform);
                outline(canvas, ghost, transform);
                self.draw_text(
                    canvas,
                    &entry.name,
                    ghost.x + 6.0,
                    ghost,
                    theme().ink,
                    transform,
                );
            }
        }

        outline(canvas, self.rect, transform);
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Key {
                key, pressed: true, ..
            } => {
                if matches!(
                    key,
                    Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown | Key::Enter
                ) {
                    let reply = self.presets.input(event, state);
                    if let Some(i) = self.chose_preset.take() {
                        return self.load(i, state);
                    }
                    return reply;
                }
                let reply = self.search.input(event, state);
                if self.search.text() != self.query {
                    self.query = self.search.text().to_owned();
                    self.rebuild_presets(state);
                    return Reply::Handled;
                }
                reply
            }
            RawKind::PointerDown { pos, button, .. } => {
                self.pointer_down(pos, button, event, state)
            }
            RawKind::PointerMove { pos, .. } => self.pointer_move(pos, event, state),
            RawKind::PointerUp { .. } => {
                self.press = None;
                if let Some(drag) = self.drag.take() {
                    return self.drop_preset(drag, state);
                }
                let _ = self.search.input(event, state);
                self.forward(event, state)
            }
            RawKind::Wheel { .. } => self.forward(event, state),
            _ => Reply::Ignored,
        }
    }

    fn update(&mut self, state: &EditorState) -> Update {
        self.catch_up(state).into()
    }
}

impl PresetBrowser {
    /// Catch up with the store and the editor; true to repaint whole.
    fn catch_up(&mut self, state: &EditorState) -> bool {
        let outside = !still_seen(&self.seen, state);
        if outside {
            let now = seen(state);
            if now.2 != self.seen.2 {
                // A save's (or a failure's) message, for the footer too.
                self.shared.borrow_mut().message.clone_from(&now.2);
            }
            self.seen = now;
        }
        let select = {
            let mut s = self.shared.borrow_mut();
            if !std::mem::take(&mut s.stale) && !outside {
                return false;
            }
            s.select.take()
        };
        if let Some(place) = select {
            self.place = place;
        }
        // A renamed or deleted folder may have taken the shown place away.
        let rows = self.shared.borrow().place_rows();
        if !rows.contains(&PlaceRow::Place(self.place.clone())) {
            self.place = Place::User(None);
        }
        self.rebuild(state);
        true
    }
}
