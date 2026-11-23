//! Preset storage and the preset field.
//!
//! A [`PresetStore`] holds the user's presets as text
//! ([`silkscreen_model::Preset::to_text`]) in one level of folders; factory presets are
//! read-only text embedded in the plugin (design/presets.md). The browser that
//! lists both is [`crate::PresetBrowser`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use silkscreen_core::tiny_skia::{
    FillRule, LineCap, Paint, PathBuilder, PixmapMut, Stroke, Transform,
};
use silkscreen_core::{Rect, WidgetState};
use silkscreen_text::Style;

use crate::EditorState;
use crate::theme::{RADIUS, Rgba, TEXT_SIZE, color, rounded, theme};
use crate::tooltip::Lettering;
use crate::widget::EditorWidget;

pub use silkscreen_model::preset::EXT;

/// Whether a file `name` is one the editor imports: a preset
/// (`.preset`) or a library (`.presetbank`), in any case.
pub fn is_import_file(name: &str) -> bool {
    let name = name.to_lowercase();
    [EXT, crate::library::EXT].iter().any(|ext| {
        name.strip_suffix(ext)
            .is_some_and(|stem| stem.ends_with('.'))
    })
}

/// What preset fields say when an import fails.
pub fn import_failed(e: impl std::fmt::Display) -> String {
    format!("Couldn't import: {e}")
}

/// What preset fields say when an export fails.
pub fn export_failed(e: impl std::fmt::Display) -> String {
    format!("Couldn't export: {e}")
}

/// Where a user preset lives: a folder (one level deep; `None` is the
/// root, shown as "Uncategorised") and its name (design/presets.md).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresetPath {
    pub folder: Option<String>,
    pub name: String,
}

impl PresetPath {
    /// `name` at the root.
    pub fn root(name: &str) -> Self {
        Self {
            folder: None,
            name: name.to_owned(),
        }
    }

    /// `name` in `folder`.
    pub fn new(folder: Option<&str>, name: &str) -> Self {
        Self {
            folder: folder.map(str::to_owned),
            name: name.to_owned(),
        }
    }
}

impl std::fmt::Display for PresetPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.folder {
            Some(folder) => write!(f, "{folder}/{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// A folder and its presets' names, as [`PresetStore::tree`] lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// `None` for the root.
    pub name: Option<String>,
    pub presets: Vec<String>,
}

/// What a new folder is called before it's renamed.
pub const NEW_FOLDER: &str = "New Folder";

/// Where the user's presets live: a folder of files natively, browser
/// storage on the web, a map in tests. Presets are addressed by
/// [`PresetPath`], in one level of folders (design/presets.md).
///
/// Mutating calls check names with [`valid_name`] and folder names with
/// [`valid_folder_name`]; errors name the preset or folder.
pub trait PresetStore {
    /// The root first, then every folder (empty ones too); folders and
    /// presets in case-insensitive order.
    fn tree(&self) -> Vec<Folder>;
    fn read(&self, path: &PresetPath) -> Option<String>;
    /// Create or replace; a missing folder is created.
    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String>;
    fn remove(&mut self, path: &PresetPath) -> Result<(), String>;
    /// Make an empty folder; refuses one that exists.
    fn add_folder(&mut self, name: &str) -> Result<(), String>;
    /// Refuses when `to` exists.
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String>;
    /// Delete a folder and every preset in it.
    fn remove_folder(&mut self, name: &str) -> Result<(), String>;

    /// Why the store can't keep presets right now, if it can't (the web
    /// store's failed writes, say), for the browser's status line.
    fn problem(&self) -> Option<String> {
        None
    }

    /// Whether `path` holds a preset.
    fn exists(&self, path: &PresetPath) -> bool {
        self.read(path).is_some()
    }

    /// Rename a preset within its folder; refuses a taken name.
    fn rename(&mut self, from: &PresetPath, to: &str) -> Result<(), String> {
        let to = PresetPath {
            folder: from.folder.clone(),
            name: valid_name(to)?,
        };
        relocate(self, from, &to)
    }

    /// Move a preset to `folder` (`None`: the root), keeping its name;
    /// refuses when the folder has one of that name.
    fn move_to(&mut self, from: &PresetPath, folder: Option<&str>) -> Result<(), String> {
        let folder = folder.map(valid_folder_name).transpose()?;
        let to = PresetPath {
            folder,
            name: from.name.clone(),
        };
        relocate(self, from, &to)
    }

    /// Make a uniquely named empty folder ("New Folder", "New Folder 2",
    /// ...) and return its name.
    fn create_folder(&mut self) -> Result<String, String> {
        let taken: Vec<String> = self
            .tree()
            .into_iter()
            .filter_map(|f| f.name.map(|n| n.to_lowercase()))
            .collect();
        // Among the first `taken.len() + 1` candidates, one is free.
        let name = std::iter::once(NEW_FOLDER.to_owned())
            .chain((2..=taken.len() + 1).map(|n| format!("{NEW_FOLDER} {n}")))
            .find(|n| !taken.contains(&n.to_lowercase()))
            .expect("an unused name");
        self.add_folder(&name)?;
        Ok(name)
    }
}

/// Move `from` to `to` by copy and remove; refuses a taken `to`.
fn relocate<S: PresetStore + ?Sized>(
    store: &mut S,
    from: &PresetPath,
    to: &PresetPath,
) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    if store.exists(to) {
        return Err(format!("a preset called \"{to}\" already exists"));
    }
    let text = store
        .read(from)
        .ok_or_else(|| format!("no preset \"{from}\""))?;
    store.write(to, &text)?;
    store.remove(from)
}

/// A preset name: trimmed, non-empty, and safe as a file name.
pub fn valid_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("a preset needs a name".into());
    }
    if n.chars()
        .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
        || n.starts_with('.')
    {
        return Err(format!(
            "\"{n}\" can't be a preset name (no / \\ : or leading dot)"
        ));
    }
    Ok(n.to_owned())
}

/// The root's name in the browser, which no folder may take.
pub const UNCATEGORISED: &str = "Uncategorised";

/// A folder name: as [`valid_name`], and not "Uncategorised".
pub fn valid_folder_name(name: &str) -> Result<String, String> {
    let n = valid_name(name).map_err(|e| e.replace("preset", "folder"))?;
    if n.eq_ignore_ascii_case(UNCATEGORISED) {
        return Err(format!("\"{n}\" is the root's name; choose another"));
    }
    Ok(n)
}

/// A preset path whose names are valid.
pub fn valid_path(path: &PresetPath) -> Result<PresetPath, String> {
    Ok(PresetPath {
        folder: path.folder.as_deref().map(valid_folder_name).transpose()?,
        name: valid_name(&path.name)?,
    })
}

/// Case-insensitive order, then exact, so the order is total.
pub fn sort_names(names: &mut [String]) {
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
}

/// What a host's [`PresetIo`] reports, once a file has been read or
/// written (design/presets.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoEvent {
    /// A file's text: a preset, or a library of them.
    Imported(String),
    /// A file was written, by its name.
    Exported(String),
    /// Reading or writing failed: the whole message, for the status line.
    Failed(String),
}

/// A host's file services beyond the store (design/presets.md): Export and
/// Import on the web and natively, Share on the web. Given with
/// `EditorState::set_preset_io`, the browser's footer offers them.
/// Files are chosen and read or written asynchronously (a file picker, a
/// Save panel, a dropped browser `File`), so outcomes wait as
/// [`IoEvent`]s the editor polls each tick. A cancelled dialog reports
/// nothing.
pub trait PresetIo {
    /// Offer `text` as a file called `file_name` (a download, a Save
    /// panel).
    fn export(&self, file_name: &str, text: &str);
    /// Ask the user for a preset or library file; its text arrives as
    /// [`IoEvent::Imported`].
    fn import(&self);
    /// Files dropped on the editor (the web host's handles): take the
    /// preset and library ones, whose texts arrive as events. False if
    /// none were taken.
    fn import_dropped(&self, _handles: &[u32]) -> bool {
        false
    }
    /// Whether [`share`](Self::share) is offered.
    fn can_share(&self) -> bool {
        false
    }
    /// Make a link that loads `text`, and put it on the clipboard.
    fn share(&self, _text: &str) -> Result<(), String> {
        Err("sharing isn't available here".into())
    }
    /// The next outcome, if one is waiting.
    fn take_event(&self) -> Option<IoEvent>;
}

/// Presets in memory (tests, and hosts with nowhere to save).
#[derive(Debug)]
pub struct MemoryStore(BTreeMap<Option<String>, BTreeMap<String, String>>);

impl Default for MemoryStore {
    fn default() -> Self {
        Self(BTreeMap::from([(None, BTreeMap::new())]))
    }
}

impl MemoryStore {
    fn has_folder(&self, name: &str) -> bool {
        self.0.contains_key(&Some(name.to_owned()))
    }
}

impl PresetStore for MemoryStore {
    fn tree(&self) -> Vec<Folder> {
        let mut folders: Vec<Folder> = self
            .0
            .iter()
            .map(|(name, presets)| {
                let mut names: Vec<String> = presets.keys().cloned().collect();
                sort_names(&mut names);
                Folder {
                    name: name.clone(),
                    presets: names,
                }
            })
            .collect();
        folders.sort_by(|a, b| match (&a.name, &b.name) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, _) => std::cmp::Ordering::Less,
            (_, None) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)),
        });
        folders
    }
    fn read(&self, path: &PresetPath) -> Option<String> {
        self.0.get(&path.folder)?.get(&path.name).cloned()
    }
    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String> {
        let path = valid_path(path)?;
        self.0
            .entry(path.folder)
            .or_default()
            .insert(path.name, text.to_owned());
        Ok(())
    }
    fn remove(&mut self, path: &PresetPath) -> Result<(), String> {
        self.0
            .get_mut(&path.folder)
            .and_then(|f| f.remove(&path.name))
            .map(|_| ())
            .ok_or_else(|| format!("no preset \"{path}\""))
    }
    fn add_folder(&mut self, name: &str) -> Result<(), String> {
        let name = valid_folder_name(name)?;
        if self.has_folder(&name) {
            return Err(format!("a folder called \"{name}\" already exists"));
        }
        self.0.insert(Some(name), BTreeMap::new());
        Ok(())
    }
    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = valid_folder_name(to)?;
        if !self.has_folder(from) {
            return Err(format!("no folder \"{from}\""));
        }
        if from == to {
            return Ok(());
        }
        if self.has_folder(&to) {
            return Err(format!("a folder called \"{to}\" already exists"));
        }
        let presets = self.0.remove(&Some(from.to_owned())).unwrap_or_default();
        self.0.insert(Some(to), presets);
        Ok(())
    }
    fn remove_folder(&mut self, name: &str) -> Result<(), String> {
        self.0
            .remove(&Some(name.to_owned()))
            .map(|_| ())
            .ok_or_else(|| format!("no folder \"{name}\""))
    }
}

/// A store shared between the editor and each browser it opens.
pub(crate) struct SharedStore(pub Rc<RefCell<Box<dyn PresetStore>>>);

impl PresetStore for SharedStore {
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
    fn exists(&self, path: &PresetPath) -> bool {
        self.0.borrow().exists(path)
    }
    fn rename(&mut self, from: &PresetPath, to: &str) -> Result<(), String> {
        self.0.borrow_mut().rename(from, to)
    }
    fn move_to(&mut self, from: &PresetPath, folder: Option<&str>) -> Result<(), String> {
        self.0.borrow_mut().move_to(from, folder)
    }
    fn create_folder(&mut self) -> Result<String, String> {
        self.0.borrow_mut().create_folder()
    }
    fn problem(&self) -> Option<String> {
        self.0.borrow().problem()
    }
}

/// What a preset field shows: the current preset's name ("Init" if
/// none), `*` after it once modified, or a brief message instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FieldText {
    pub name: Option<String>,
    pub modified: bool,
    pub flash: Option<String>,
}

/// Room for the modified marker before the ›.
const MARKER_W: f32 = 8.0;

/// The preset field (design/presets.md): a box with ‹ and › at its ends and
/// the current preset's name between them.
pub(crate) struct PresetField {
    /// What it showed when last drawn.
    text: FieldText,
    lettering: Lettering,
}

impl PresetField {
    pub fn new() -> Self {
        Self {
            text: FieldText::default(),
            lettering: Lettering::shared(),
        }
    }
}

/// A small chevron pointing left (`dir` -1) or right (1) at `(cx, cy)`.
fn arrow(canvas: &mut PixmapMut<'_>, cx: f32, cy: f32, dir: f32, ink: Rgba, transform: Transform) {
    let half = 3.0;
    let mut pb = PathBuilder::new();
    pb.move_to(cx - dir * half / 2.0, cy - half);
    pb.line_to(cx + dir * half / 2.0, cy);
    pb.line_to(cx - dir * half / 2.0, cy + half);
    if let Some(path) = pb.finish() {
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color(ink));
        let stroke = Stroke {
            width: 1.2,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

impl EditorWidget for PresetField {
    fn update(&mut self, state: &EditorState) -> bool {
        let now = state.preset_text();
        if *now == self.text {
            return false;
        }
        self.text.clone_from(now);
        true
    }

    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        // The box, as a drop-down's but without its chevron.
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        if let Some(path) = rounded(rect, RADIUS) {
            paint.set_color(color(theme().background));
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
            if state.hover {
                paint.set_color(color(theme().hover));
                canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
            paint.set_color(color(theme().border));
            let stroke = Stroke {
                width: 1.0 / transform.sx,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        let aw = silkscreen_model::preset_arrow_w(rect.w);
        let cy = rect.center().y;
        arrow(canvas, rect.x + aw / 2.0, cy, -1.0, theme().ink, transform);
        arrow(
            canvas,
            rect.right() - aw / 2.0,
            cy,
            1.0,
            theme().ink,
            transform,
        );

        let text = &self.text;
        // The modified marker keeps its place even when the name is cut.
        let marked = text.flash.is_none() && text.modified;
        let label = match (&text.flash, &text.name) {
            (Some(flash), _) => flash.clone(),
            (None, name) => name.as_deref().unwrap_or("Init").to_owned(),
        };
        let face = &self.lettering.face;
        let (ascent, descent) = face.line_metrics(TEXT_SIZE);
        let style = Style {
            face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        let (left, mut right) = (rect.x + aw, rect.right() - aw);
        let baseline = rect.center().y + f32::midpoint(ascent, descent);
        let mut lettering = self.lettering.text.borrow_mut();
        if marked {
            let star = MARKER_W;
            right -= star;
            lettering.draw_clipped(
                canvas,
                "*",
                (right + 1.0, baseline),
                &style,
                transform,
                Some([right, rect.y, right + star, rect.bottom()]),
            );
        }
        lettering.draw_clipped(
            canvas,
            &label,
            (left + 2.0, baseline),
            &style,
            transform,
            Some([left, rect.y, right, rect.bottom()]),
        );
    }
}

/// Whether preset `name` shows whole in a preset field of `rect` at view
/// `scale`: between its arrows, with room left for the modified marker,
/// as the field draws it (it clips what doesn't fit).
pub fn name_fits(rect: Rect, name: &str, scale: f32) -> bool {
    let lettering = Lettering::shared();
    let aw = silkscreen_model::preset_arrow_w(rect.w);
    let room = rect.w - 2.0 * aw - MARKER_W - 2.0;
    let width = lettering
        .text
        .borrow_mut()
        .width(&lettering.face, name, TEXT_SIZE, scale);
    width <= room
}

#[cfg(test)]
mod tests {
    #[test]
    fn import_files_are_presets_and_libraries_in_any_case() {
        assert!(super::is_import_file("Saw.preset"));
        assert!(super::is_import_file("MINE.PRESETBANK"));
        assert!(!super::is_import_file("notes.txt"));
        assert!(!super::is_import_file("preset"), "an extension, not a name");
    }

    use super::*;

    fn at(folder: &str, name: &str) -> PresetPath {
        PresetPath::new(Some(folder), name)
    }

    #[test]
    fn the_memory_store_keeps_folders() {
        let mut s = MemoryStore::default();
        assert_eq!(
            s.tree(),
            [Folder {
                name: None,
                presets: vec![]
            }],
            "the root is always there"
        );
        s.write(&PresetPath::root("b"), "B").unwrap();
        s.write(&PresetPath::root("A"), "A").unwrap();
        s.write(&at("pads", "Warm"), "W").unwrap();
        assert_eq!(s.create_folder().unwrap(), NEW_FOLDER);
        let tree = s.tree();
        assert_eq!(tree[0].presets, ["A", "b"]);
        let names: Vec<_> = tree.iter().map(|f| f.name.as_deref()).collect();
        assert_eq!(
            names,
            [None, Some("New Folder"), Some("pads")],
            "case-insensitive"
        );

        s.rename(&at("pads", "Warm"), "Warmer").unwrap();
        s.move_to(&at("pads", "Warmer"), None).unwrap();
        assert_eq!(s.read(&PresetPath::root("Warmer")).as_deref(), Some("W"));
        assert!(
            s.rename(&PresetPath::root("A"), "b")
                .unwrap_err()
                .contains("already exists")
        );
        assert!(s.move_to(&PresetPath::root("A"), Some("..")).is_err());
        s.rename_folder("pads", "Pads").unwrap();
        assert!(
            s.rename_folder("Pads", "New Folder")
                .unwrap_err()
                .contains("already exists")
        );
        s.write(&at("Pads", "X"), "X").unwrap();
        s.remove_folder("Pads").unwrap();
        assert!(s.read(&at("Pads", "X")).is_none(), "gone with its folder");
        assert!(s.remove(&at("Pads", "X")).unwrap_err().contains("Pads/X"));
    }

    #[test]
    fn names_and_folder_names_are_checked() {
        let mut s = MemoryStore::default();
        assert!(s.write(&PresetPath::root(" "), "").is_err());
        assert!(s.write(&at("a/b", "x"), "").is_err());
        assert!(s.add_folder("uncategorised").unwrap_err().contains("root"));
        assert!(valid_folder_name("..").unwrap_err().contains("folder"));
        assert_eq!(valid_name("  Lead  ").unwrap(), "Lead");
    }
}
