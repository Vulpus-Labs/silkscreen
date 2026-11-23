//! A user's presets as one file (design/presets.md): a `.presetbank`
//! holds every user preset and folder, to move a library between the
//! web, a plugin, or two machines.
//!
//! ```text
//! # silkscreen preset library
//! library = 1
//! product = Tone
//! version = 0.1.0
//!
//! [Leads/Bright Saw]
//! # silkscreen preset
//! format = 1
//! product = Tone
//! version = 0.1.0
//! name = Bright Saw
//! p1 = 0.8  # Level
//!
//! [Init Copy]
//! ...
//!
//! [Empty Folder/]
//! ```
//!
//! Each `[folder/name]` (`[name]` at the root) heads a section whose body
//! is the preset's text as the store holds it; `[folder/]` keeps an empty
//! folder. A valid preset never has a line starting with `[`, so the
//! sections can't be confused with their bodies.
//!
//! `product` and `version` say which plugin, at which version, exported the
//! library (design/presets.md). Each section keeps its own, as it was saved: a
//! preset saved before presets carried them has none.
//!
//! Importing merges: folders match existing ones ignoring case, a clash
//! keeps both ("Name 2"), and a section that doesn't parse or isn't
//! validly named is skipped while the rest import.

use std::fmt::Write as _;

use silkscreen_model::{AppVersion, Preset};

use crate::presets::{PresetPath, PresetStore, valid_folder_name, valid_name};

/// A library file's first line, which tells it from a single preset.
pub const HEADER: &str = "# silkscreen preset library";
/// The wrapper's version: the only one this build writes and reads.
pub const VERSION: u32 = 1;
/// A library file's extension.
pub const EXT: &str = "presetbank";
/// What an exported library is called.
pub const FILE_NAME: &str = "User presets.presetbank";

/// A parsed library: its presets' paths and texts, unchecked, and the
/// empty folders it declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    /// The plugin that exported it, and its version (design/presets.md); none
    /// in a library exported before libraries carried them.
    pub product: Option<String>,
    pub version: Option<String>,
    pub folders: Vec<String>,
    pub presets: Vec<(PresetPath, String)>,
}

/// Whether `text` is a library rather than a single preset: its first
/// non-blank line is [`HEADER`].
pub fn is_library(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .is_some_and(|l| l == HEADER)
}

/// Every user preset in `store` as a library, in `tree()` order, and how
/// many presets it holds; marked as `app`'s, if given (design/presets.md).
pub fn export_library(store: &dyn PresetStore, app: Option<AppVersion>) -> (String, usize) {
    let mut out = format!("{HEADER}\nlibrary = {VERSION}\n");
    if let Some(app) = app {
        let _ = write!(
            out,
            "product = {}\nversion = {}\n",
            app.product, app.version
        );
    }
    let mut count = 0;
    for folder in store.tree() {
        if folder.presets.is_empty() {
            if let Some(name) = &folder.name {
                let _ = write!(out, "\n[{name}/]\n");
            }
            continue;
        }
        for name in &folder.presets {
            let path = PresetPath::new(folder.name.as_deref(), name);
            let Some(text) = store.read(&path) else {
                continue;
            };
            let _ = write!(out, "\n[{path}]\n{}\n", text.trim_end());
            count += 1;
        }
    }
    (out, count)
}

/// A section heading's path: `[folder/name]`, `[name]`, or `[folder/]`
/// (a folder, with an empty name).
fn heading(line: &str) -> Option<PresetPath> {
    let inner = line.trim_end().strip_prefix('[')?.strip_suffix(']')?;
    Some(match inner.split_once('/') {
        Some((folder, name)) => PresetPath::new(Some(folder), name),
        None => PresetPath::root(inner),
    })
}

/// Read a library. Only its header is checked here; each section is
/// checked as it's imported. Errors name the line.
pub fn parse_library(text: &str) -> Result<Library, String> {
    if !is_library(text) {
        return Err(format!(
            "not a preset library (its first line isn't \"{HEADER}\")"
        ));
    }
    let mut library = Library::default();
    let mut version = None;
    let mut section: Option<(PresetPath, Vec<&str>)> = None;
    let finish = |library: &mut Library, section: Option<(PresetPath, Vec<&str>)>| {
        if let Some((path, body)) = section {
            let body = body.join("\n");
            library
                .presets
                .push((path, format!("{}\n", body.trim_matches('\n'))));
        }
    };
    for (n, line) in text.lines().enumerate() {
        if let Some(path) = heading(line) {
            finish(&mut library, section.take());
            if path.name.is_empty() {
                if let Some(folder) = path.folder {
                    library.folders.push(folder);
                }
            } else {
                section = Some((path, Vec::new()));
            }
            continue;
        }
        if let Some((_, body)) = &mut section {
            body.push(line);
            continue;
        }
        // Before the first section: comments, blanks and the version.
        let bare = line.split('#').next().unwrap_or("").trim();
        if bare.is_empty() {
            continue;
        }
        match bare.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("product", v)) => library.product = Some(v.to_owned()),
            Some(("version", v)) => library.version = Some(v.to_owned()),
            Some(("library", v)) => {
                if v.parse::<u32>().ok() != Some(VERSION) {
                    return Err(format!(
                        "line {}: preset library version \"{v}\" is not supported (this build reads version {VERSION})",
                        n + 1
                    ));
                }
                version = Some(VERSION);
            }
            _ => {
                return Err(format!(
                    "line {}: expected `library = {VERSION}` or a `[folder/name]` section, found \"{bare}\"",
                    n + 1
                ));
            }
        }
    }
    finish(&mut library, section);
    version.ok_or("no `library = ...` line")?;
    Ok(library)
}

/// Whether `app` can import `library`, by its header's product and
/// version (design/presets.md; a library with no version was exported by
/// [`UNVERSIONED`](silkscreen_model::UNVERSIONED)).
///
/// # Errors
/// Why not, naming both.
pub fn check_library(library: &Library, app: AppVersion) -> Result<(), String> {
    app.accepts(library.product.as_deref(), library.version.as_deref())
}

/// How an import went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryImport {
    /// Presets written, renamed ones included.
    pub added: usize,
    /// Presets written under a new name because theirs was taken.
    pub renamed: usize,
    /// Why each skipped section was skipped, naming it.
    pub skipped: Vec<String>,
}

impl LibraryImport {
    /// "Imported 12 presets (3 renamed, 1 skipped: …)", for the status
    /// line; the first skip's reason is given.
    pub fn summary(&self) -> String {
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut s = format!("Imported {} preset{}", self.added, plural(self.added));
        let mut notes = Vec::new();
        if self.renamed > 0 {
            notes.push(format!("{} renamed", self.renamed));
        }
        if let Some(first) = self.skipped.first() {
            notes.push(format!("{} skipped: {first}", self.skipped.len()));
        }
        if !notes.is_empty() {
            let _ = write!(s, " ({})", notes.join(", "));
        }
        s
    }
}

/// `name`, or the first of "name 2", "name 3", … not in `taken`
/// (lower-cased).
fn free_name(name: &str, taken: &[String]) -> String {
    let free = |n: &str| !taken.contains(&n.to_lowercase());
    if free(name) {
        return name.to_owned();
    }
    // Among the first `taken.len() + 1` candidates, one is free.
    (2..=taken.len() + 2)
        .map(|i| format!("{name} {i}"))
        .find(|n| free(n))
        .expect("an unused name")
}

/// Each folder's spelling (`None`: the root) and its presets' names,
/// lower-cased, as an import goes.
type Taken = Vec<(Option<String>, Vec<String>)>;

/// The index in `folders` of folder `name` (`None`: the root), matched
/// ignoring case and made in `store` if missing.
fn folder_index(
    store: &mut dyn PresetStore,
    folders: &mut Taken,
    name: Option<&str>,
) -> Result<usize, String> {
    let Some(name) = name else {
        return Ok(folders.iter().position(|(f, _)| f.is_none()).unwrap_or(0));
    };
    let name = valid_folder_name(name)?;
    if let Some(i) = folders.iter().position(|(f, _)| {
        f.as_deref()
            .is_some_and(|f| f.to_lowercase() == name.to_lowercase())
    }) {
        return Ok(i);
    }
    store.add_folder(&name)?;
    folders.push((Some(name), Vec::new()));
    Ok(folders.len() - 1)
}

/// Merge `library` into `store` (design/presets.md): folders matched ignoring
/// case, clashes kept as "Name 2", bad sections skipped.
///
/// With `app`, a section whose own product or version `app` can't import
/// is skipped (design/presets.md); the library's header is checked before, by
/// [`check_library`].
pub fn import_library(
    store: &mut dyn PresetStore,
    library: &Library,
    app: Option<AppVersion>,
) -> LibraryImport {
    let mut report = LibraryImport::default();
    let mut folders: Taken = store
        .tree()
        .into_iter()
        .map(|f| (f.name, f.presets.iter().map(|p| p.to_lowercase()).collect()))
        .collect();

    if !folders.iter().any(|(f, _)| f.is_none()) {
        folders.insert(0, (None, Vec::new()));
    }

    for folder in &library.folders {
        if let Err(e) = folder_index(store, &mut folders, Some(folder)) {
            report
                .skipped
                .push(format!("folder \u{201c}{folder}\u{201d}: {e}"));
        }
    }
    for (path, text) in &library.presets {
        let skip = |e: String| format!("\u{201c}{path}\u{201d}: {e}");
        let name = match valid_name(&path.name) {
            Ok(n) => n,
            Err(e) => {
                report.skipped.push(skip(e));
                continue;
            }
        };
        match Preset::parse(text) {
            Err(e) => {
                report.skipped.push(skip(e));
                continue;
            }
            Ok(preset) => {
                if let Some(Err(e)) =
                    app.map(|a| a.accepts(preset.product.as_deref(), preset.version.as_deref()))
                {
                    report.skipped.push(skip(e));
                    continue;
                }
            }
        }
        let i = match folder_index(store, &mut folders, path.folder.as_deref()) {
            Ok(i) => i,
            Err(e) => {
                report.skipped.push(skip(e));
                continue;
            }
        };
        let (folder, taken) = &mut folders[i];
        let free = free_name(&name, taken);
        match store.write(&PresetPath::new(folder.as_deref(), &free), text) {
            Ok(()) => {
                report.added += 1;
                if free != name {
                    report.renamed += 1;
                }
                taken.push(free.to_lowercase());
            }
            Err(e) => report.skipped.push(skip(e)),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::MemoryStore;

    fn preset(name: &str) -> String {
        format!("# silkscreen preset\nformat = 1\nname = {name}\np1 = 0.5  # Level\n")
    }

    fn store_with(entries: &[(Option<&str>, &str)], empty: &[&str]) -> MemoryStore {
        let mut s = MemoryStore::default();
        for (folder, name) in entries {
            s.write(&PresetPath::new(*folder, name), &preset(name))
                .unwrap();
        }
        for f in empty {
            s.add_folder(f).unwrap();
        }
        s
    }

    #[test]
    fn a_library_round_trips() {
        let mut from = store_with(
            &[
                (None, "Root"),
                (Some("Leads"), "Saw"),
                (Some("Leads"), "Square"),
                (Some("Pads"), "Warm"),
            ],
            &["Empty"],
        );
        // Metadata with `#` and `[` survives.
        let odd =
            "# silkscreen preset\nformat = 1\nname = Odd\ncomment = [not] a # heading\np1 = 1\n";
        from.write(&PresetPath::new(Some("Pads"), "Odd"), odd)
            .unwrap();
        let (text, count) = export_library(&from, None);
        assert_eq!(count, 5);
        assert!(is_library(&text));
        assert!(text.contains("\n[Leads/Saw]\n# silkscreen preset\n"));
        assert!(text.contains("\n[Empty/]\n"));

        let library = parse_library(&text).unwrap();
        assert_eq!(library.folders, ["Empty"]);
        assert_eq!(library.presets.len(), 5);
        let mut to = MemoryStore::default();
        let report = import_library(&mut to, &library, None);
        assert_eq!(
            report,
            LibraryImport {
                added: 5,
                renamed: 0,
                skipped: vec![]
            }
        );
        assert_eq!(to.tree(), from.tree());
        assert_eq!(
            to.read(&PresetPath::new(Some("Pads"), "Odd")).as_deref(),
            Some(odd)
        );
        assert_eq!(report.summary(), "Imported 5 presets");
    }

    #[test]
    fn clashes_keep_both_and_folders_match_ignoring_case() {
        let mut store = store_with(
            &[
                (Some("Leads"), "Saw"),
                (Some("Leads"), "saw 2"),
                (None, "Root"),
            ],
            &[],
        );
        let text = format!(
            "{HEADER}\nlibrary = 1\n[leads/Saw]\n{}[LEADS/New]\n{}[Root]\n{}[Root]\n{}",
            preset("Saw"),
            preset("New"),
            preset("Root"),
            preset("Root"),
        );
        let report = import_library(&mut store, &parse_library(&text).unwrap(), None);
        assert_eq!((report.added, report.renamed), (4, 3));
        let tree = store.tree();
        assert_eq!(tree.len(), 2, "no second Leads folder");
        assert_eq!(tree[0].presets, ["Root", "Root 2", "Root 3"]);
        assert_eq!(tree[1].name.as_deref(), Some("Leads"));
        assert_eq!(tree[1].presets, ["New", "Saw", "saw 2", "Saw 3"]);
        assert_eq!(report.summary(), "Imported 4 presets (3 renamed)");
    }

    #[test]
    fn bad_sections_are_skipped_and_the_rest_import() {
        let text = format!(
            "{HEADER}\nlibrary = 1\n[Good]\n{}[Bad]\nnot a preset\n[../Escape]\n{}[Uncategorised/X]\n{}[.hidden]\n{}",
            preset("Good"),
            preset("Escape"),
            preset("X"),
            preset("hidden"),
        );
        let mut store = MemoryStore::default();
        let report = import_library(&mut store, &parse_library(&text).unwrap(), None);
        assert_eq!(report.added, 1);
        assert_eq!(report.skipped.len(), 4);
        assert!(
            report.skipped[0].starts_with("\u{201c}Bad\u{201d}: line 1"),
            "{report:?}"
        );
        assert!(
            report
                .summary()
                .starts_with("Imported 1 preset (4 skipped: \u{201c}Bad\u{201d}")
        );
        assert_eq!(store.tree().len(), 1, "no folders made for skipped presets");
    }

    #[test]
    fn a_library_carries_and_checks_the_app_version() {
        let app = AppVersion {
            product: "Tone",
            version: "0.3.1",
        };
        let from = store_with(&[(None, "A")], &[]);
        let (text, _) = export_library(&from, Some(app));
        assert!(
            text.starts_with(&format!(
                "{HEADER}\nlibrary = 1\nproduct = Tone\nversion = 0.3.1\n"
            )),
            "{text}"
        );
        let library = parse_library(&text).unwrap();
        assert_eq!(library.version.as_deref(), Some("0.3.1"));
        assert_eq!(check_library(&library, app), Ok(()));
        let later = AppVersion {
            version: "0.4.0",
            ..app
        };
        assert!(check_library(&library, later).is_err());
        // A section of another version is skipped, the rest imported.
        let mixed = format!(
            "{HEADER}\nlibrary = 1\n[Old]\n{}[Ok]\n{}",
            preset("Old"),
            preset("Ok").replace("format = 1\n", "format = 1\nversion = 0.3.0\n"),
        );
        let mut store = MemoryStore::default();
        let report = import_library(&mut store, &parse_library(&mixed).unwrap(), Some(app));
        assert_eq!(report.added, 1, "{report:?}");
        assert!(report.skipped[0].contains("0.1.0"), "{report:?}");
    }

    #[test]
    fn headers_and_versions_are_checked() {
        assert!(!is_library(&preset("A")));
        assert!(
            parse_library(&preset("A"))
                .unwrap_err()
                .contains("not a preset library")
        );
        let e = parse_library(&format!("{HEADER}\nlibrary = 2\n")).unwrap_err();
        assert!(e.contains("line 2") && e.contains("version \"2\""), "{e}");
        assert!(parse_library(HEADER).unwrap_err().contains("library = "));
        let e = parse_library(&format!("{HEADER}\nlibrary = 1\nstray\n")).unwrap_err();
        assert!(e.contains("line 3"), "{e}");
        let empty = parse_library(&format!("\n  {HEADER}\nlibrary = 1  # v1\n")).unwrap();
        assert_eq!(empty, Library::default());
        assert_eq!(
            import_library(&mut MemoryStore::default(), &empty, None).summary(),
            "Imported 0 presets"
        );
    }
}
