//! A folder of preset files: one `<name>.preset` text file
//! per preset ([`silkscreen::Preset`]'s format), in one level of
//! subfolders under a per-plugin folder. Files at the top are the root's
//! ("Uncategorised").
//!
//! Every mutating call resolves its path and refuses anything outside
//! the store's folder, so a symlinked subfolder can't lead writes
//! elsewhere (design/presets.md). Subfolders that are symlinks aren't listed.

use std::fs;
use std::path::{Path, PathBuf};

use silkscreen::presets::{sort_names, valid_folder_name, valid_path};
use silkscreen::{Folder, PresetPath, PresetStore};

use silkscreen::presets::EXT;

/// The user's presets in `dir`, created on first save.
#[derive(Clone, Debug)]
pub struct DirStore {
    dir: PathBuf,
}

impl DirStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The platform's usual place for a plugin's presets:
    /// `~/Library/Audio/Presets/<vendor>/<plugin>` on macOS,
    /// `%APPDATA%\<vendor>\<plugin>\Presets` on Windows, and
    /// `$XDG_DATA_HOME/<plugin>/presets` (or `~/.local/share/...`)
    /// elsewhere. `None` if the home folder can't be found.
    pub fn default_for(vendor: &str, plugin: &str) -> Option<Self> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let dir = if cfg!(target_os = "macos") {
            env("HOME")?
                .join("Library/Audio/Presets")
                .join(vendor)
                .join(plugin)
        } else if cfg!(windows) {
            env("APPDATA")?.join(vendor).join(plugin).join("Presets")
        } else {
            env("XDG_DATA_HOME")
                .or_else(|| env("HOME").map(|h| h.join(".local/share")))?
                .join(plugin)
                .join("presets")
        };
        Some(Self::new(dir))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn folder_dir(&self, folder: Option<&str>) -> PathBuf {
        folder.map_or_else(|| self.dir.clone(), |f| self.dir.join(f))
    }

    fn file(&self, path: &PresetPath) -> PathBuf {
        self.folder_dir(path.folder.as_deref())
            .join(format!("{}.{EXT}", path.name))
    }

    /// The store's folder, created and resolved.
    fn root(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|e| describe(&e, &self.dir))?;
        self.dir.canonicalize().map_err(|e| describe(&e, &self.dir))
    }

    /// Refuse `dir` (which must exist) unless it resolves inside the
    /// store's folder.
    fn ensure_within(&self, dir: &Path) -> Result<(), String> {
        let root = self.root()?;
        let real = dir.canonicalize().map_err(|e| describe(&e, dir))?;
        if real.starts_with(&root) {
            Ok(())
        } else {
            Err(format!(
                "{} is outside the preset folder {}",
                dir.display(),
                root.display()
            ))
        }
    }

    /// An existing subfolder, not a symlink, inside the store.
    fn existing_folder(&self, name: &str) -> Result<PathBuf, String> {
        let dir = self.folder_dir(Some(name));
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {
                self.ensure_within(&dir)?;
                Ok(dir)
            }
            Ok(_) => Err(format!("\"{name}\" is not a folder")),
            Err(_) => Err(format!("no folder \"{name}\"")),
        }
    }

    fn presets_in(dir: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == EXT))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
            .filter(|n| !n.starts_with('.'))
            .collect();
        sort_names(&mut names);
        names
    }
}

fn describe(e: &std::io::Error, path: &Path) -> String {
    format!("{}: {e}", path.display())
}

impl PresetStore for DirStore {
    fn tree(&self) -> Vec<Folder> {
        let mut folders = vec![Folder {
            name: None,
            presets: Self::presets_in(&self.dir),
        }];
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| valid_folder_name(n).is_ok())
            .collect();
        sort_names(&mut names);
        folders.extend(names.into_iter().map(|name| Folder {
            presets: Self::presets_in(&self.dir.join(&name)),
            name: Some(name),
        }));
        folders
    }

    fn read(&self, path: &PresetPath) -> Option<String> {
        let path = valid_path(path).ok()?;
        fs::read_to_string(self.file(&path)).ok()
    }

    fn write(&mut self, path: &PresetPath, text: &str) -> Result<(), String> {
        let path = valid_path(path)?;
        self.root()?;
        let dir = self.folder_dir(path.folder.as_deref());
        if let Some(folder) = &path.folder
            && fs::symlink_metadata(&dir).is_err()
        {
            self.add_folder(folder)?;
        }
        self.ensure_within(&dir)?;
        // Write beside, then rename over: a crash never leaves half a file.
        let file = self.file(&path);
        let tmp = dir.join(format!(".{}.{EXT}.tmp", path.name));
        fs::write(&tmp, text).map_err(|e| describe(&e, &tmp))?;
        fs::rename(&tmp, &file).map_err(|e| describe(&e, &file))
    }

    fn remove(&mut self, path: &PresetPath) -> Result<(), String> {
        let path = valid_path(path)?;
        let file = self.file(&path);
        if !file.is_file() {
            return Err(format!("no preset \"{path}\""));
        }
        self.ensure_within(&self.folder_dir(path.folder.as_deref()))?;
        fs::remove_file(&file).map_err(|e| describe(&e, &file))
    }

    fn add_folder(&mut self, name: &str) -> Result<(), String> {
        let name = valid_folder_name(name)?;
        self.root()?;
        let dir = self.folder_dir(Some(&name));
        if fs::symlink_metadata(&dir).is_ok() {
            return Err(format!("a folder called \"{name}\" already exists"));
        }
        fs::create_dir(&dir).map_err(|e| describe(&e, &dir))
    }

    fn rename_folder(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = valid_folder_name(to)?;
        let a = self.existing_folder(from)?;
        if from == to {
            return Ok(());
        }
        let b = self.folder_dir(Some(&to));
        // A case-only rename is the same folder on case-insensitive disks.
        if fs::symlink_metadata(&b).is_ok() && !from.eq_ignore_ascii_case(&to) {
            return Err(format!("a folder called \"{to}\" already exists"));
        }
        fs::rename(&a, &b).map_err(|e| describe(&e, &a))
    }

    fn remove_folder(&mut self, name: &str) -> Result<(), String> {
        let dir = self.existing_folder(name)?;
        fs::remove_dir_all(&dir).map_err(|e| describe(&e, &dir))
    }

    fn rename(&mut self, from: &PresetPath, to: &str) -> Result<(), String> {
        let from = valid_path(from)?;
        let to = valid_path(&PresetPath {
            folder: from.folder.clone(),
            name: to.to_owned(),
        })?;
        self.relocate(&from, &to)
    }

    fn move_to(&mut self, from: &PresetPath, folder: Option<&str>) -> Result<(), String> {
        let from = valid_path(from)?;
        let to = valid_path(&PresetPath {
            folder: folder.map(str::to_owned),
            name: from.name.clone(),
        })?;
        self.relocate(&from, &to)
    }
}

impl DirStore {
    /// Rename a file within the store: both folders must be inside it,
    /// `to`'s must exist (or be the root), and `to` must be free.
    fn relocate(&mut self, from: &PresetPath, to: &PresetPath) -> Result<(), String> {
        if from == to {
            return Ok(());
        }
        let (a, b) = (self.file(from), self.file(to));
        if !a.is_file() {
            return Err(format!("no preset \"{from}\""));
        }
        let case_only = from.folder == to.folder && from.name.eq_ignore_ascii_case(&to.name);
        if b.exists() && !case_only {
            return Err(format!("a preset called \"{to}\" already exists"));
        }
        if let Some(folder) = &to.folder {
            self.existing_folder(folder)?;
        }
        self.ensure_within(&self.folder_dir(from.folder.as_deref()))?;
        self.ensure_within(&self.folder_dir(to.folder.as_deref()))?;
        fs::rename(&a, &b).map_err(|e| describe(&e, &a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("silkscreen-presets-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn root(name: &str) -> PresetPath {
        PresetPath::root(name)
    }

    fn at(folder: &str, name: &str) -> PresetPath {
        PresetPath::new(Some(folder), name)
    }

    #[test]
    fn saves_lists_renames_and_deletes_files() {
        let dir = temp("flat");
        let mut store = DirStore::new(&dir);
        assert_eq!(
            store.tree()[0].presets,
            Vec::<String>::new(),
            "no folder yet"
        );
        store.write(&root("bass"), "name = bass\n").unwrap();
        store.write(&root("Anthem"), "name = Anthem\n").unwrap();
        assert_eq!(
            store.tree()[0].presets,
            ["Anthem", "bass"],
            "case-insensitive order"
        );
        assert_eq!(store.read(&root("bass")).as_deref(), Some("name = bass\n"));
        store.rename(&root("bass"), "Bass 2").unwrap();
        assert!(
            store
                .rename(&root("Anthem"), "Bass 2")
                .unwrap_err()
                .contains("already exists")
        );
        store.remove(&root("Anthem")).unwrap();
        assert_eq!(store.tree()[0].presets, ["Bass 2"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folders_hold_presets_and_empty_ones_are_kept() {
        let dir = temp("folders");
        let mut store = DirStore::new(&dir);
        store.write(&at("Leads", "Saw"), "name = Saw\n").unwrap();
        assert!(
            dir.join("Leads/Saw.preset").is_file(),
            "a missing folder is made"
        );
        assert_eq!(store.create_folder().unwrap(), "New Folder");
        assert_eq!(store.create_folder().unwrap(), "New Folder 2");
        store.write(&root("Top"), "name = Top\n").unwrap();
        let tree = store.tree();
        let names: Vec<_> = tree.iter().map(|f| f.name.as_deref()).collect();
        assert_eq!(
            names,
            [
                None,
                Some("Leads"),
                Some("New Folder"),
                Some("New Folder 2")
            ]
        );
        assert_eq!(tree[0].presets, ["Top"]);
        assert_eq!(tree[1].presets, ["Saw"]);
        assert!(tree[2].presets.is_empty());

        store
            .move_to(&at("Leads", "Saw"), Some("New Folder"))
            .unwrap();
        assert_eq!(
            store.read(&at("New Folder", "Saw")).as_deref(),
            Some("name = Saw\n")
        );
        store.move_to(&at("New Folder", "Saw"), None).unwrap();
        store.write(&at("Leads", "Saw"), "name = Saw 2\n").unwrap();
        assert!(
            store
                .move_to(&root("Saw"), Some("Leads"))
                .unwrap_err()
                .contains("already exists"),
            "a move refuses a taken name"
        );
        assert!(
            store
                .move_to(&root("Saw"), Some("Nowhere"))
                .unwrap_err()
                .contains("no folder")
        );

        store.rename_folder("Leads", "Lead Sounds").unwrap();
        assert!(store.read(&at("Lead Sounds", "Saw")).is_some());
        assert!(
            store
                .rename_folder("New Folder", "Lead Sounds")
                .unwrap_err()
                .contains("already exists")
        );
        store.remove_folder("Lead Sounds").unwrap();
        assert!(
            !dir.join("Lead Sounds").exists(),
            "removed with its presets"
        );
        assert!(
            store
                .remove_folder("Lead Sounds")
                .unwrap_err()
                .contains("no folder")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_names_are_refused() {
        let dir = temp("names");
        let mut store = DirStore::new(&dir);
        assert!(store.write(&root("../escape"), "x").is_err());
        assert!(store.write(&at("..", "x"), "x").is_err());
        assert!(store.write(&root(".hidden"), "x").is_err());
        assert!(store.add_folder("a/b").is_err());
        assert!(
            store
                .add_folder("Uncategorised")
                .unwrap_err()
                .contains("root")
        );
        assert!(store.read(&root("../etc/passwd")).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_folder_cannot_lead_writes_outside() {
        let dir = temp("symlink");
        let outside = temp("symlink-outside");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("Victim.preset"), "name = Victim\n").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("Out")).unwrap();

        let mut store = DirStore::new(&dir);
        assert!(
            store
                .tree()
                .iter()
                .all(|f| f.name.as_deref() != Some("Out")),
            "symlinked folders aren't listed"
        );
        assert!(
            store
                .write(&at("Out", "Planted"), "x")
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            store
                .remove(&at("Out", "Victim"))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            store
                .remove_folder("Out")
                .unwrap_err()
                .contains("not a folder")
        );
        store.write(&root("Mine"), "name = Mine\n").unwrap();
        assert!(store.move_to(&root("Mine"), Some("Out")).is_err());
        assert!(outside.join("Victim.preset").is_file(), "untouched");
        assert!(!outside.join("Planted.preset").exists());
        assert!(!outside.join("Mine.preset").exists());
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn default_locations_follow_the_platform() {
        let Some(store) = DirStore::default_for("Acme Audio", "Tone") else {
            return;
        };
        let dir = store.dir().to_string_lossy().replace('\\', "/");
        if cfg!(target_os = "macos") {
            assert!(
                dir.ends_with("Library/Audio/Presets/Acme Audio/Tone"),
                "{dir}"
            );
        } else if cfg!(windows) {
            assert!(dir.ends_with("Acme Audio/Tone/Presets"), "{dir}");
        } else {
            assert!(dir.ends_with("Tone/presets"), "{dir}");
        }
    }
}
