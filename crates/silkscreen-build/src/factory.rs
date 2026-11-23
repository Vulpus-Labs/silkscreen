//! Factory preset banks (design/presets.md): a plugin's
//! `presets/factory/<Category>/<Name>.preset` files, checked and embedded
//! as a generated table.
//!
//! Call from `build.rs`:
//!
//! ```no_run
//! silkscreen_build::FactoryBank::new("presets/factory").build();
//! ```
//!
//! and pull the table in with:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/factory_presets.rs"));
//! // pub static FACTORY: &[silkscreen::FactoryPreset] = ...;
//! ```
//!
//! The build checks what it can without the plugin's param table: each
//! file parses, its `name` is its file's, its `category` (if any) is its
//! folder's, nothing sits loose outside a category folder, and no two
//! presets share a name. Param ids and ranges are the plugin's tests' to
//! check, with `silkscreen_harness::assert_factory_bank`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use silkscreen_model::Preset;

use crate::BuildError;

use silkscreen_model::preset::EXT;

/// One checked factory preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryEntry {
    pub category: String,
    pub name: String,
    pub path: PathBuf,
}

/// A factory bank's folder, to check and embed.
#[derive(Clone, Debug)]
pub struct FactoryBank {
    dir: PathBuf,
    out_dir: Option<PathBuf>,
}

impl FactoryBank {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            out_dir: None,
        }
    }

    /// Write the generated table here instead of `OUT_DIR`.
    #[must_use]
    pub fn out_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.out_dir = Some(dir.into());
        self
    }

    /// [`generate`](Self::generate), panicking with the error's message on
    /// failure. Meant for `build.rs`.
    pub fn build(&self) -> Vec<FactoryEntry> {
        match self.generate() {
            Ok(entries) => entries,
            Err(e) => panic!("\n\nsilkscreen-build: {e}\n\n"),
        }
    }

    /// Check the bank and write `factory_presets.rs`, telling Cargo to
    /// rerun when the folder or any preset changes.
    pub fn generate(&self) -> Result<Vec<FactoryEntry>, BuildError> {
        let entries = self.scan()?;
        let out = match &self.out_dir {
            Some(d) => d.clone(),
            None => std::env::var_os("OUT_DIR")
                .map(PathBuf::from)
                .ok_or(BuildError::NoOutDir)?,
        };
        if self.out_dir.is_none() {
            println!("cargo:rerun-if-changed={}", self.dir.display());
            for e in &entries {
                println!("cargo:rerun-if-changed={}", e.path.display());
            }
        }
        let path = out.join("factory_presets.rs");
        std::fs::write(&path, codegen(&entries)).map_err(|e| BuildError::Io {
            path,
            message: e.to_string(),
        })?;
        Ok(entries)
    }

    /// Check the bank without writing anything: its entries, sorted by
    /// category and then name, ignoring case.
    pub fn scan(&self) -> Result<Vec<FactoryEntry>, BuildError> {
        let mut entries = Vec::new();
        for category in sorted_dir(&self.dir)? {
            let cat_name = file_name(&category);
            if category.is_dir() {
                // Folders inside a category, and categories' other files, are
                // mistakes too.
                for file in sorted_dir(&category)? {
                    if file.is_dir() {
                        return Err(problem(
                            &file,
                            "folders inside a category aren't read; banks are one level deep",
                        ));
                    }
                    if file.extension().is_some_and(|x| x == EXT) {
                        entries.push(check(&file, &cat_name)?);
                    }
                }
            } else if category.extension().is_some_and(|x| x == EXT) {
                return Err(problem(
                    &category,
                    "a preset outside a category folder; move it into presets/factory/<Category>/",
                ));
            }
        }
        entries.sort_by_key(|e| (e.category.to_lowercase(), e.name.to_lowercase()));
        for (i, e) in entries.iter().enumerate() {
            if let Some(first) = entries[..i]
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(&e.name))
            {
                return Err(problem(
                    &e.path,
                    &format!(
                        "another factory preset is called \"{}\" ({}); rename one",
                        e.name,
                        first.path.display()
                    ),
                ));
            }
        }
        Ok(entries)
    }
}

/// Check one preset file in category `category`.
fn check(path: &Path, category: &str) -> Result<FactoryEntry, BuildError> {
    let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    let preset = Preset::parse(&text).map_err(|e| problem(path, &e))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    if preset.name != stem {
        return Err(problem(
            path,
            &format!(
                "its `name` is \"{}\"; make it the file's name, \"{stem}\"",
                preset.name
            ),
        ));
    }
    if let Some(c) = preset.category.as_deref().filter(|c| *c != category) {
        return Err(problem(
            path,
            &format!(
                "its `category` is \"{c}\" but it is in the \"{category}\" folder; make them agree"
            ),
        ));
    }
    Ok(FactoryEntry {
        category: category.to_owned(),
        name: stem,
        path: path.to_owned(),
    })
}

fn problem(path: &Path, problem: &str) -> BuildError {
    BuildError::FactoryPreset {
        path: path.to_owned(),
        problem: problem.to_owned(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `dir`'s entries, hidden ones skipped, in name order.
fn sorted_dir(dir: &Path) -> Result<Vec<PathBuf>, BuildError> {
    let io = |e: std::io::Error| BuildError::Io {
        path: dir.to_owned(),
        message: e.to_string(),
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| !file_name(p).starts_with('.'))
        .collect();
    paths.sort();
    Ok(paths)
}

/// The generated table.
fn codegen(entries: &[FactoryEntry]) -> String {
    let mut out = String::from(
        "// Generated by silkscreen-build from the factory preset bank. Do not edit.\n\n\
         /// The factory presets, by category and then name.\n\
         pub static FACTORY: &[::silkscreen::FactoryPreset] = &[\n",
    );
    for e in entries {
        let abs = e.path.canonicalize().unwrap_or_else(|_| e.path.clone());
        let _ = write!(
            out,
            "    ::silkscreen::FactoryPreset {{\n        category: {:?},\n        name: {:?},\n        text: include_str!({:?}),\n    }},\n",
            e.category,
            e.name,
            abs.to_string_lossy()
        );
    }
    out.push_str("];\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh bank folder holding `files` (path under it, text).
    fn bank(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("silkscreen-factory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn error(name: &str, files: &[(&str, &str)]) -> String {
        let dir = bank(name, files);
        let e = FactoryBank::new(&dir).scan().unwrap_err().to_string();
        std::fs::remove_dir_all(&dir).unwrap();
        e
    }

    #[test]
    fn a_good_bank_is_sorted_and_generated() {
        let dir = bank(
            "good",
            &[
                ("Lead/Saw.preset", "name = Saw\np1 = 1\n"),
                ("bass/Sub.preset", "name = Sub\ncategory = bass\n"),
                ("bass/Acid.preset", "name = Acid\n"),
                ("bass/notes.txt", "ignored"),
            ],
        );
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let entries = FactoryBank::new(&dir).out_dir(&out).generate().unwrap();
        let names: Vec<_> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.name))
            .collect();
        assert_eq!(names, ["bass/Acid", "bass/Sub", "Lead/Saw"]);
        let code = std::fs::read_to_string(out.join("factory_presets.rs")).unwrap();
        assert!(code.contains("pub static FACTORY"), "{code}");
        assert!(code.contains("name: \"Acid\""), "{code}");
        assert!(code.contains("Saw.preset\")"), "{code}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_parse_error_names_the_file_and_line() {
        let e = error("parse", &[("Bass/Sub.preset", "name = Sub\np1 = loud\n")]);
        let file = Path::new("Bass").join("Sub.preset");
        assert!(
            e.contains(&*file.to_string_lossy()) && e.contains("line 2"),
            "{e}"
        );
    }

    #[test]
    fn a_name_unlike_the_file_is_refused() {
        let e = error("name", &[("Bass/Sub.preset", "name = Other\n")]);
        assert!(e.contains("Sub.preset") && e.contains("\"Other\""), "{e}");
    }

    #[test]
    fn a_category_unlike_the_folder_is_refused() {
        let e = error(
            "category",
            &[("Bass/Sub.preset", "name = Sub\ncategory = Pad\n")],
        );
        assert!(
            e.contains("Sub.preset") && e.contains("\"Pad\"") && e.contains("\"Bass\""),
            "{e}"
        );
    }

    #[test]
    fn a_loose_preset_is_refused() {
        let e = error("loose", &[("Sub.preset", "name = Sub\n")]);
        assert!(
            e.contains("Sub.preset") && e.contains("category folder"),
            "{e}"
        );
    }

    #[test]
    fn nested_folders_are_refused() {
        let e = error("nested", &[("Bass/Deep/Sub.preset", "name = Sub\n")]);
        assert!(e.contains("Deep") && e.contains("one level"), "{e}");
    }

    #[test]
    fn repeated_names_are_refused() {
        let e = error(
            "repeat",
            &[
                ("Bass/Sub.preset", "name = Sub\n"),
                ("Pad/sub.preset", "name = sub\n"),
            ],
        );
        assert!(
            e.contains("sub.preset") && e.contains("another factory preset"),
            "{e}"
        );
    }

    #[test]
    fn a_missing_folder_is_named() {
        let e = FactoryBank::new("/nonexistent/presets/factory")
            .scan()
            .unwrap_err()
            .to_string();
        assert!(e.contains("/nonexistent/presets/factory"), "{e}");
    }
}
