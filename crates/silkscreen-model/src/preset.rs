//! Presets as text: one per file, human-readable, keyed by each
//! param's stable id so presets survive a reordered param table.
//!
//! ```text
//! # silkscreen preset
//! format = 1
//! product = Tone
//! version = 0.1.0
//! name = Squelchy Bass
//! category = Bass
//! author = Example Audio
//! p1 = 0.8  # Level
//! p3 = 2  # Wave: Pulse
//! ```
//!
//! A preset is a whole state: params it doesn't mention load at their
//! defaults, and ids it has that the table doesn't are ignored (a preset
//! from a later version). `#` starts a comment; the comments written are
//! for people and are ignored when read.
//!
//! `format` versions the layout (design/presets.md): a file without it is
//! format 1, and formats this build doesn't know are refused. `product`
//! and `version` say which plugin, at which version, wrote it (design/presets.md),
//! so a later version can tell a preset that needs upgrading; a file
//! without them was written before presets carried them. `name`,
//! `category`, `author` and `comment` take the rest of their line as is,
//! `#` included; every param is written, so a preset never changes sound
//! if a default does.

use std::fmt::Write as _;

use crate::param::ParamSpec;

/// The format this build writes, and the only one it reads.
pub const FORMAT: u32 = 1;

/// A preset file's extension: `<name>.preset`.
pub const EXT: &str = "preset";

/// The text keys that take the rest of their line.
const TEXT_KEYS: [&str; 6] = [
    "product", "version", "name", "category", "author", "comment",
];

/// Which plugin, at which version, wrote a preset or a library (design/presets.md):
/// its product name, e.g. `"Tone"`, and its version, e.g.
/// `"0.1.0"`, as its CLAP descriptor gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppVersion {
    pub product: &'static str,
    pub version: &'static str,
}

/// The version a preset or library without one was written by: every build
/// before presets carried versions was 0.1.0 (design/presets.md).
pub const UNVERSIONED: &str = "0.1.0";

/// A semantic version's major and minor numbers: `"1.4.2"` (or
/// `"1.4.2-beta"`) is `(1, 4)`.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.trim().split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    Some((major, minor))
}

impl AppVersion {
    /// Whether this build can import a preset or library marked `product`
    /// at `version` (design/presets.md): the same product, if it names one, and
    /// the same major and minor version. A patch release changes nothing a
    /// preset holds, so presets move freely between patch versions; any
    /// change to what a preset means is a minor (or major) release, and
    /// its presets are refused here until an upgrade reads them. A file
    /// with no version was written by [`UNVERSIONED`].
    ///
    /// # Errors
    /// Why not, naming both products or versions.
    pub fn accepts(&self, product: Option<&str>, version: Option<&str>) -> Result<(), String> {
        if let Some(product) = product.map(str::trim)
            && !product.eq_ignore_ascii_case(self.product)
        {
            return Err(format!("it is a {product} preset, not {}", self.product));
        }
        let theirs = version.map_or(UNVERSIONED, str::trim);
        let Some(their) = major_minor(theirs) else {
            return Err(format!(
                "its version \u{201c}{theirs}\u{201d} isn't major.minor.patch"
            ));
        };
        let ours = major_minor(self.version);
        if ours != Some(their) {
            let (major, minor) = ours.unwrap_or_default();
            return Err(format!(
                "it was made by {} {theirs}; this is {}, which reads {major}.{minor}.x",
                self.product, self.version
            ));
        }
        Ok(())
    }
}

/// A named set of param values, by stable id, with optional metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    /// The plugin that wrote it, e.g. `"Tone"` (design/presets.md); none if
    /// it was written before presets carried it, or by hand.
    pub product: Option<String>,
    /// That plugin's version, e.g. `"0.1.0"`: what a later version reads
    /// to know whether the preset needs upgrading.
    pub version: Option<String>,
    pub name: String,
    /// A factory preset's category: its folder in the bank.
    pub category: Option<String>,
    pub author: Option<String>,
    /// One line about the sound.
    pub comment: Option<String>,
    pub values: Vec<(u32, f32)>,
}

/// `text` on one line: line breaks become spaces, and the ends are
/// trimmed.
fn one_line(text: &str) -> String {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

impl Preset {
    /// The preset holding `values` (plain, one per spec) for `specs`.
    pub fn capture(name: &str, specs: &[ParamSpec], values: impl IntoIterator<Item = f32>) -> Self {
        Self {
            name: name.to_owned(),
            values: specs.iter().map(|s| s.id).zip(values).collect(),
            ..Self::default()
        }
    }

    /// Plain values for `specs`, in table order: the preset's where it has
    /// one (clamped to range), each spec's default otherwise.
    pub fn values_for(&self, specs: &[ParamSpec]) -> Vec<f32> {
        specs
            .iter()
            .map(|s| {
                self.values
                    .iter()
                    .find(|(id, _)| *id == s.id)
                    .map_or(s.default, |(_, v)| s.clamp(*v))
            })
            .collect()
    }

    /// The preset as text, each value commented with its param's name (and
    /// label, for discrete params) from `specs`. Metadata with line breaks
    /// is written on one line.
    pub fn to_text(&self, specs: &[ParamSpec]) -> String {
        let mut out = String::from("# silkscreen preset\n");
        let _ = writeln!(out, "format = {FORMAT}");
        for (key, value) in [("product", &self.product), ("version", &self.version)] {
            if let Some(v) = value.as_deref().map(one_line).filter(|v| !v.is_empty()) {
                let _ = writeln!(out, "{key} = {v}");
            }
        }
        let _ = writeln!(out, "name = {}", one_line(&self.name));
        for (key, value) in [
            ("category", &self.category),
            ("author", &self.author),
            ("comment", &self.comment),
        ] {
            if let Some(v) = value.as_deref().map(one_line).filter(|v| !v.is_empty()) {
                let _ = writeln!(out, "{key} = {v}");
            }
        }
        for (id, value) in &self.values {
            let _ = write!(out, "p{id} = {value}");
            if let Some(spec) = specs.iter().find(|s| s.id == *id) {
                if spec.kind.is_discrete() {
                    let _ = write!(out, "  # {}: {}", spec.name, spec.format(*value));
                } else {
                    let _ = write!(out, "  # {}", spec.name);
                }
            }
            out.push('\n');
        }
        out
    }

    /// Read a preset written by [`to_text`](Self::to_text) (or by hand).
    /// Errors name the line and what's wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut preset = Self::default();
        let mut name = None;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!(
                    "line {}: expected `key = value`, found \"{line}\"",
                    n + 1
                ));
            };
            let (key, value) = (key.trim(), value.trim());
            if TEXT_KEYS.contains(&key) {
                // Text may itself contain '#': take it from the raw line.
                let full = raw
                    .split_once('=')
                    .map_or(value, |(_, v)| v.trim())
                    .to_owned();
                match key {
                    "product" => preset.product = Some(full),
                    "version" => preset.version = Some(full),
                    "name" => name = Some(full),
                    "category" => preset.category = Some(full),
                    "author" => preset.author = Some(full),
                    _ => preset.comment = Some(full),
                }
            } else if key == "format" {
                if value.parse::<u32>().ok() != Some(FORMAT) {
                    return Err(format!(
                        "line {}: preset format \"{value}\" is not supported (this build reads format {FORMAT})",
                        n + 1
                    ));
                }
            } else if let Some(id) = key.strip_prefix('p').and_then(|d| d.parse::<u32>().ok()) {
                let v: f32 = value
                    .parse()
                    .map_err(|_| format!("line {}: \"{value}\" is not a number", n + 1))?;
                preset.values.push((id, v));
            } else {
                return Err(format!(
                    "line {}: unknown key \"{key}\" (expected `format`, `product`, `version`, `name`, `category`, `author`, `comment` or `p<id>`)",
                    n + 1
                ));
            }
        }
        preset.name = name.ok_or("no `name = ...` line")?;
        Ok(preset)
    }
}

/// A read-only preset embedded in a plugin: one of a factory bank's
/// files, `presets/factory/<category>/<name>.preset` (design/presets.md), as
/// `silkscreen-build` generates them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactoryPreset {
    pub category: &'static str,
    pub name: &'static str,
    pub text: &'static str,
}

/// Everything wrong with factory `bank` against `specs`, one message per
/// problem naming the preset: text that doesn't parse, a name or category
/// unlike the entry's, an id the table doesn't have, a value out of its
/// param's range, a repeated name, or text that doesn't survive a round
/// trip. Empty when the bank is sound.
pub fn check_factory(bank: &[FactoryPreset], specs: &[ParamSpec]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, entry) in bank.iter().enumerate() {
        let at = format!("factory preset {}/{}", entry.category, entry.name);
        if bank[..i]
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(entry.name))
        {
            problems.push(format!("{at}: another preset has this name"));
        }
        let preset = match Preset::parse(entry.text) {
            Ok(p) => p,
            Err(e) => {
                problems.push(format!("{at}: {e}"));
                continue;
            }
        };
        if preset.name != entry.name {
            problems.push(format!(
                "{at}: its `name` is \"{}\"; make it the file's name",
                preset.name
            ));
        }
        if let Some(c) = preset.category.as_deref().filter(|c| *c != entry.category) {
            problems.push(format!(
                "{at}: its `category` is \"{c}\" but it is in the \"{}\" folder",
                entry.category
            ));
        }
        for &(id, value) in &preset.values {
            match specs.iter().find(|s| s.id == id) {
                None => problems.push(format!("{at}: p{id} is not a param id")),
                Some(spec) if spec.clamp(value) != value => problems.push(format!(
                    "{at}: p{id} ({}) = {value} is outside {}..={}",
                    spec.name, spec.min, spec.max
                )),
                Some(_) => {}
            }
        }
        if Preset::parse(&preset.to_text(specs)).as_ref() != Ok(&preset) {
            problems.push(format!("{at}: does not survive a round trip through text"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_build_accepts_its_own_minor_version_alone() {
        let app = AppVersion {
            product: "Tone",
            version: "0.2.3",
        };
        // Any patch, either way, and any product's spelling.
        for v in ["0.2.0", "0.2.3", "0.2.9", "0.2.10-beta"] {
            assert_eq!(app.accepts(Some("TONE"), Some(v)), Ok(()), "{v}");
        }
        assert_eq!(app.accepts(None, Some("0.2.1")), Ok(()), "no product named");
        // Another minor or major, older or newer.
        for v in ["0.1.9", "0.3.0", "1.2.3"] {
            let e = app.accepts(Some("tone"), Some(v)).unwrap_err();
            assert!(e.contains(v) && e.contains("0.2.x"), "{e}");
        }
        let e = app.accepts(Some("Gain"), Some("0.2.3")).unwrap_err();
        assert!(e.contains("Gain preset"), "{e}");
        assert!(app.accepts(None, Some("two")).is_err());
        // No version: written before versions, by 0.1.0.
        assert!(app.accepts(None, None).is_err());
        let first = AppVersion {
            product: "Tone",
            version: "0.1.4",
        };
        assert_eq!(first.accepts(None, None), Ok(()));
    }

    #[test]
    fn product_and_version_round_trip_after_the_format() {
        let p = Preset {
            product: Some("Tone".into()),
            version: Some("0.2.1".into()),
            ..Preset::capture("Bell", &SPECS, [0.5, 0.0, 0.0])
        };
        let text = p.to_text(&SPECS);
        assert!(
            text.starts_with(
                "# silkscreen preset\nformat = 1\nproduct = Tone\nversion = 0.2.1\nname = Bell\n"
            ),
            "{text}"
        );
        assert_eq!(Preset::parse(&text).unwrap(), p);
        // Written before presets carried them: neither.
        let old = Preset::parse("# silkscreen preset\nformat = 1\nname = Old\n").unwrap();
        assert_eq!((old.product, old.version), (None, None));
    }

    use super::*;

    static SPECS: [ParamSpec; 3] = [
        ParamSpec::new("Level", 0.0, 1.0, 0.5).id(1),
        ParamSpec::stepped("Wave", &["Saw", "Square", "Pulse"], 0).id(3),
        ParamSpec::new("Fine", -50.0, 50.0, 0.0).bipolar().id(8),
    ];

    #[test]
    fn round_trips_through_text() {
        let p = Preset::capture("Squelchy Bass", &SPECS, [0.8, 2.0, -12.5]);
        let text = p.to_text(&SPECS);
        assert!(text.contains("p3 = 2  # Wave: Pulse"), "{text}");
        assert_eq!(Preset::parse(&text).unwrap(), p);
    }

    #[test]
    fn missing_params_default_and_unknown_ids_are_ignored() {
        let p = Preset::parse("name = Old\np1 = 2.0\np99 = 4\n").unwrap();
        assert_eq!(p.values_for(&SPECS), [1.0, 0.0, 0.0], "clamped, defaulted");
    }

    #[test]
    fn names_may_hold_hashes_and_errors_name_the_line() {
        let p = Preset::parse("name = Lead #2\n").unwrap();
        assert_eq!(p.name, "Lead #2");
        let e = Preset::parse("name = X\np1 = loud\n").unwrap_err();
        assert!(e.contains("line 2") && e.contains("\"loud\""), "{e}");
        assert!(Preset::parse("p1 = 1\n").unwrap_err().contains("name"));
    }

    #[test]
    fn metadata_round_trips_and_is_written_after_the_format() {
        let p = Preset {
            category: Some("Bass".into()),
            author: Some("Example Audio".into()),
            comment: Some("Wheel opens #1 filter".into()),
            ..Preset::capture("Squelchy Bass", &SPECS, [0.8, 2.0, -12.5])
        };
        let text = p.to_text(&SPECS);
        assert!(
            text.starts_with(
                "# silkscreen preset\nformat = 1\nname = Squelchy Bass\ncategory = Bass\nauthor = Example Audio\ncomment = Wheel opens #1 filter\np1 = "
            ),
            "{text}"
        );
        assert_eq!(Preset::parse(&text).unwrap(), p);
    }

    #[test]
    fn absent_metadata_writes_no_lines() {
        let text = Preset::capture("Plain", &SPECS, [0.5, 0.0, 0.0]).to_text(&SPECS);
        for key in ["category", "author", "comment"] {
            assert!(!text.contains(key), "{text}");
        }
    }

    #[test]
    fn line_breaks_in_metadata_are_flattened() {
        let p = Preset {
            comment: Some("two\nlines\r\n".into()),
            ..Preset::capture("A\nB", &SPECS, [0.5, 0.0, 0.0])
        };
        let back = Preset::parse(&p.to_text(&SPECS)).unwrap();
        assert_eq!(back.name, "A B");
        assert_eq!(back.comment.as_deref(), Some("two lines"));
    }

    #[test]
    fn a_0064_file_without_a_format_line_still_loads() {
        let p = Preset::parse("# silkscreen preset\nname = Old\np1 = 0.25  # Level\n").unwrap();
        assert_eq!(p.name, "Old");
        assert_eq!(p.category, None);
        assert_eq!(p.values, [(1, 0.25)]);
    }

    fn entry(category: &'static str, name: &'static str, text: &'static str) -> FactoryPreset {
        FactoryPreset {
            category,
            name,
            text,
        }
    }

    #[test]
    fn a_sound_factory_bank_has_no_problems() {
        let bank = [
            entry("Bass", "Sub", "name = Sub\ncategory = Bass\np1 = 0.2\n"),
            entry("Lead", "Saw", "name = Saw\np3 = 1\n"),
        ];
        assert_eq!(check_factory(&bank, &SPECS), Vec::<String>::new());
    }

    #[test]
    fn factory_problems_name_the_preset() {
        let bank = [
            entry("Bass", "Sub", "name = Sub\np1 = loud\n"),
            entry("Bass", "Wrong", "name = Other\n"),
            entry("Bass", "Cat", "name = Cat\ncategory = Lead\n"),
            entry("Bass", "Ids", "name = Ids\np99 = 1\np1 = 7\n"),
            entry("Lead", "sub", "name = sub\n"),
        ];
        let problems = check_factory(&bank, &SPECS);
        let has = |what: &str, text: &str| {
            problems
                .iter()
                .any(|p| p.contains(what) && p.contains(text))
        };
        assert!(has("Bass/Sub", "line 2"), "{problems:#?}");
        assert!(has("Bass/Wrong", "\"Other\""), "{problems:#?}");
        assert!(has("Bass/Cat", "\"Lead\""), "{problems:#?}");
        assert!(has("Bass/Ids", "p99 is not a param id"), "{problems:#?}");
        assert!(has("Bass/Ids", "outside"), "{problems:#?}");
        assert!(has("Lead/sub", "another preset"), "{problems:#?}");
        assert_eq!(problems.len(), 6, "{problems:#?}");
    }

    #[test]
    fn unknown_formats_are_refused_naming_the_format_and_line() {
        let e = Preset::parse("# silkscreen preset\nformat = 2\nname = New\n").unwrap_err();
        assert!(
            e.contains("line 2") && e.contains("\"2\"") && e.contains("format 1"),
            "{e}"
        );
        assert!(Preset::parse("format = one\nname = X\n").is_err());
    }
}
