//! The web's preset store (design/presets.md), its target-independent
//! half: a [`MemoryStore`] cache that answers the editor synchronously,
//! and a journal of the keys each change touched.
//!
//! Flushing turns the journal into [`Write`]s from the cache as it is
//! then, so several changes to one key write once, and a rename is a put
//! and a delete. A failed flush puts its keys back to be tried again. The
//! `IndexedDB` side (`idb.rs`, wasm only) hydrates the cache at boot and
//! carries the writes out.
//!
//! Keys in the database: a preset at the root is its name; one in a
//! folder is `folder/name` (names can't hold `/`, so this is unambiguous).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use silkscreen::presets::valid_path;
use silkscreen::{Folder, MemoryStore, PresetPath, PresetStore};

/// Something the journal records as changed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Preset(PresetPath),
    Folder(String),
}

/// One change for the database to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    PutPreset { key: String, text: String },
    DeletePreset { key: String },
    PutFolder { name: String },
    DeleteFolder { name: String },
}

/// `path`'s database key.
pub fn preset_key(path: &PresetPath) -> String {
    path.to_string()
}

/// The path a database key names, if its names are valid.
pub(crate) fn parse_key(key: &str) -> Option<PresetPath> {
    let path = match key.split_once('/') {
        Some((folder, name)) => PresetPath::new(Some(folder), name),
        None => PresetPath::root(key),
    };
    valid_path(&path).ok().filter(|p| *p == path)
}

struct Inner {
    cache: MemoryStore,
    dirty: BTreeSet<Key>,
    problem: Option<String>,
    /// Called after each change, to schedule a flush.
    on_change: Option<Rc<dyn Fn()>>,
}

/// The cache and its journal. Clones share them: the editor holds one as
/// its store, the flusher another.
#[derive(Clone)]
pub struct JournaledStore(Rc<RefCell<Inner>>);

impl std::fmt::Debug for JournaledStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.0.borrow();
        f.debug_struct("JournaledStore")
            .field("dirty", &inner.dirty.len())
            .field("problem", &inner.problem)
            .finish_non_exhaustive()
    }
}

impl Default for JournaledStore {
    fn default() -> Self {
        Self::hydrate(Vec::new(), Vec::new())
    }
}

impl JournaledStore {
    /// A store holding what the database held: its `folders` and its
    /// `presets` (key, text). Entries with bad names are skipped. Nothing
    /// is dirty.
    pub fn hydrate(folders: Vec<String>, presets: Vec<(String, String)>) -> Self {
        let mut cache = MemoryStore::default();
        for name in folders {
            let _ = cache.add_folder(&name);
        }
        for (key, text) in presets {
            if let Some(path) = parse_key(&key) {
                let _ = cache.write(&path, &text);
            }
        }
        Self(Rc::new(RefCell::new(Inner {
            cache,
            dirty: BTreeSet::new(),
            problem: None,
            on_change: None,
        })))
    }

    /// Run `f` after every change (the `IndexedDB` side schedules a flush).
    pub fn set_on_change(&self, f: impl Fn() + 'static) {
        self.0.borrow_mut().on_change = Some(Rc::new(f));
    }

    /// Whether changes wait to be written.
    pub fn is_dirty(&self) -> bool {
        !self.0.borrow().dirty.is_empty()
    }

    /// Take the journal as writes from the cache as it is now: a put for
    /// what exists, a delete for what doesn't. Returns the keys too, to
    /// [`requeue`](Self::requeue) if the writes fail.
    pub fn take_writes(&self) -> (Vec<Key>, Vec<Write>) {
        let mut inner = self.0.borrow_mut();
        let keys: Vec<Key> = std::mem::take(&mut inner.dirty).into_iter().collect();
        let folders: Vec<String> = inner
            .cache
            .tree()
            .into_iter()
            .filter_map(|f| f.name)
            .collect();
        let writes = keys
            .iter()
            .map(|k| match k {
                Key::Preset(path) => match inner.cache.read(path) {
                    Some(text) => Write::PutPreset {
                        key: preset_key(path),
                        text,
                    },
                    None => Write::DeletePreset {
                        key: preset_key(path),
                    },
                },
                Key::Folder(name) if folders.contains(name) => {
                    Write::PutFolder { name: name.clone() }
                }
                Key::Folder(name) => Write::DeleteFolder { name: name.clone() },
            })
            .collect();
        (keys, writes)
    }

    /// Writes for `keys` failed with `problem`: keep them to try again,
    /// and say why.
    pub fn requeue(&self, keys: Vec<Key>, problem: String) {
        let mut inner = self.0.borrow_mut();
        inner.dirty.extend(keys);
        inner.problem = Some(problem);
    }

    /// Say why presets can't be kept (or `None` once they can again).
    pub fn set_problem(&self, problem: Option<String>) {
        self.0.borrow_mut().problem = problem;
    }

    /// Note `keys` changed and tell the flusher.
    fn touched(&self, keys: impl IntoIterator<Item = Key>) {
        let on_change = {
            let mut inner = self.0.borrow_mut();
            inner.dirty.extend(keys);
            inner.on_change.clone()
        };
        if let Some(f) = on_change {
            f();
        }
    }

    fn presets_in(&self, folder: &str) -> Vec<String> {
        self.0
            .borrow()
            .cache
            .tree()
            .into_iter()
            .find(|f| f.name.as_deref() == Some(folder))
            .map(|f| f.presets)
            .unwrap_or_default()
    }
}

impl PresetStore for JournaledStore {
    fn tree(&self) -> Vec<Folder> {
        self.0.borrow().cache.tree()
    }

    fn read(&self, path: &PresetPath) -> Option<String> {
        self.0.borrow().cache.read(path)
    }

    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String> {
        let path = valid_path(path)?;
        self.0.borrow_mut().cache.write(&path, text)?;
        let folder = path.folder.clone().map(Key::Folder);
        self.touched(folder.into_iter().chain([Key::Preset(path)]));
        Ok(())
    }

    fn remove(&mut self, path: &PresetPath) -> Result<(), String> {
        self.0.borrow_mut().cache.remove(path)?;
        self.touched([Key::Preset(path.clone())]);
        Ok(())
    }

    fn add_folder(&mut self, name: &str) -> Result<(), String> {
        self.0.borrow_mut().cache.add_folder(name)?;
        self.touched([Key::Folder(name.trim().to_owned())]);
        Ok(())
    }

    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        let presets = self.presets_in(from);
        self.0.borrow_mut().cache.rename_folder(from, to)?;
        let to = to.trim();
        let moved = presets.iter().flat_map(|n| {
            [
                Key::Preset(PresetPath::new(Some(from), n)),
                Key::Preset(PresetPath::new(Some(to), n)),
            ]
        });
        let folders = [Key::Folder(from.to_owned()), Key::Folder(to.to_owned())];
        self.touched(folders.into_iter().chain(moved).collect::<Vec<_>>());
        Ok(())
    }

    fn remove_folder(&mut self, name: &str) -> Result<(), String> {
        let presets = self.presets_in(name);
        self.0.borrow_mut().cache.remove_folder(name)?;
        let gone = presets
            .iter()
            .map(|n| Key::Preset(PresetPath::new(Some(name), n)));
        self.touched(
            std::iter::once(Key::Folder(name.to_owned()))
                .chain(gone)
                .collect::<Vec<_>>(),
        );
        Ok(())
    }

    fn problem(&self) -> Option<String> {
        self.0.borrow().problem.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(folder: &str, name: &str) -> PresetPath {
        PresetPath::new(Some(folder), name)
    }

    /// A stand-in database: what a backend holds after applying writes.
    #[derive(Default)]
    struct Fake {
        presets: std::collections::BTreeMap<String, String>,
        folders: BTreeSet<String>,
    }

    impl Fake {
        fn apply(&mut self, writes: Vec<Write>) {
            for w in writes {
                match w {
                    Write::PutPreset { key, text } => {
                        self.presets.insert(key, text);
                    }
                    Write::DeletePreset { key } => {
                        self.presets.remove(&key);
                    }
                    Write::PutFolder { name } => {
                        self.folders.insert(name);
                    }
                    Write::DeleteFolder { name } => {
                        self.folders.remove(&name);
                    }
                }
            }
        }

        /// A store hydrated from what the fake holds: a reload.
        fn reload(&self) -> JournaledStore {
            JournaledStore::hydrate(
                self.folders.iter().cloned().collect(),
                self.presets
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        }
    }

    fn flush(store: &JournaledStore, db: &mut Fake) {
        let (_, writes) = store.take_writes();
        db.apply(writes);
    }

    #[test]
    fn keys_round_trip_and_bad_ones_are_skipped() {
        assert_eq!(preset_key(&PresetPath::root("Lead")), "Lead");
        assert_eq!(preset_key(&at("Pads", "Warm")), "Pads/Warm");
        assert_eq!(parse_key("Pads/Warm"), Some(at("Pads", "Warm")));
        assert_eq!(parse_key("Lead"), Some(PresetPath::root("Lead")));
        assert_eq!(parse_key("../x"), None);
        assert_eq!(parse_key("a/b/c"), None);
        assert_eq!(parse_key(" Lead"), None, "not the name it was saved as");
    }

    #[test]
    fn every_change_survives_a_reload() {
        let mut db = Fake::default();
        let mut s = JournaledStore::default();
        s.write(&PresetPath::root("Top"), "T").unwrap();
        s.write(&at("Leads", "Saw"), "S").unwrap();
        let empty = s.create_folder().unwrap();
        flush(&s, &mut db);
        assert!(!s.is_dirty());
        assert_eq!(db.reload().tree(), s.tree());
        assert!(
            db.reload()
                .tree()
                .iter()
                .any(|f| f.name.as_deref() == Some(&*empty))
        );

        s.rename(&at("Leads", "Saw"), "Saw 2").unwrap();
        s.move_to(&PresetPath::root("Top"), Some("Leads")).unwrap();
        s.rename_folder("Leads", "Lead Sounds").unwrap();
        flush(&s, &mut db);
        assert_eq!(db.reload().tree(), s.tree());
        assert_eq!(
            db.presets.keys().collect::<Vec<_>>(),
            ["Lead Sounds/Saw 2", "Lead Sounds/Top"]
        );

        s.remove_folder("Lead Sounds").unwrap();
        s.remove_folder(&empty).unwrap();
        flush(&s, &mut db);
        assert!(db.presets.is_empty() && db.folders.is_empty());
        assert_eq!(db.reload().tree(), s.tree());
    }

    #[test]
    fn changes_to_one_key_write_once() {
        let mut s = JournaledStore::default();
        for text in ["a", "b", "c"] {
            s.write(&PresetPath::root("X"), text).unwrap();
        }
        let (_, writes) = s.take_writes();
        assert_eq!(
            writes,
            [Write::PutPreset {
                key: "X".into(),
                text: "c".into()
            }]
        );
        s.write(&PresetPath::root("Y"), "y").unwrap();
        s.remove(&PresetPath::root("Y")).unwrap();
        let (_, writes) = s.take_writes();
        assert_eq!(writes, [Write::DeletePreset { key: "Y".into() }]);
    }

    #[test]
    fn a_failed_flush_is_kept_and_reported() {
        let mut db = Fake::default();
        let mut s = JournaledStore::default();
        s.write(&PresetPath::root("X"), "1").unwrap();
        let (keys, _) = s.take_writes();
        s.requeue(keys, "QuotaExceededError".into());
        assert!(s.is_dirty());
        assert_eq!(s.problem().as_deref(), Some("QuotaExceededError"));
        s.write(&PresetPath::root("X"), "2").unwrap();
        flush(&s, &mut db);
        assert_eq!(db.presets.get("X").map(String::as_str), Some("2"));
    }

    #[test]
    fn changes_tell_the_flusher() {
        let calls = Rc::new(std::cell::Cell::new(0));
        let mut s = JournaledStore::default();
        let c = Rc::clone(&calls);
        s.set_on_change(move || c.set(c.get() + 1));
        s.write(&PresetPath::root("X"), "1").unwrap();
        s.create_folder().unwrap();
        assert!(s.write(&PresetPath::root("../x"), "1").is_err());
        assert_eq!(calls.get(), 2, "refused changes don't");
    }
}
