//! Bitmap faces for the LCD canvas mode ([design/faceplates.md]): a text source
//! becomes a static `LcdFont`, and a character the editor needs that the
//! face lacks fails the build.
//!
//! The source has a `size WIDTH HEIGHT ADVANCE` line, then one glyph a
//! line: the character (`space` for a space), then `HEIGHT` rows of `.`
//! and `#`. `//` starts a comment line.
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::error::BuildError;

/// A face to generate. From `build.rs`:
///
/// ```no_run
/// silkscreen_build::BitmapFont::new(silkscreen_build::LCD_FONT)
///     .require("0123456789.-+ HzdBms")
///     .build();
/// ```
///
/// writes `silk-5x7.rs` to `OUT_DIR`, holding `pub static FONT: LcdFont`.
#[derive(Clone, Debug)]
pub struct BitmapFont {
    path: PathBuf,
    required: String,
    core_path: String,
    out_dir: Option<PathBuf>,
}

/// A parsed face: glyphs sorted by character, rows as in `LcdFont`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFont {
    pub width: u8,
    pub height: u8,
    pub advance: u8,
    pub glyphs: Vec<(char, Vec<u8>)>,
}

impl BitmapFont {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            required: String::new(),
            core_path: "::silkscreen::core".into(),
            out_dir: None,
        }
    }

    /// Characters the editor draws. Call again to add more: typically
    /// its fixed labels and the characters its readouts can produce.
    pub fn require(mut self, chars: &str) -> Self {
        self.required.push_str(chars);
        self
    }

    /// Path through which generated code names `silkscreen-core` types.
    /// Default `::silkscreen::core`.
    pub fn core_path(mut self, path: impl Into<String>) -> Self {
        self.core_path = path.into();
        self
    }

    /// Write outputs here instead of `$OUT_DIR`.
    pub fn out_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.out_dir = Some(dir.into());
        self
    }

    /// [`generate`](Self::generate) for `build.rs`: panics with the error.
    pub fn build(&self) -> PathBuf {
        match self.generate() {
            Ok(path) => path,
            Err(e) => panic!("\n\nsilkscreen-build: {e}\n\n"),
        }
    }

    /// Read the source and check every required character.
    pub fn load(&self) -> Result<ParsedFont, BuildError> {
        let text = std::fs::read_to_string(&self.path).map_err(|e| BuildError::Io {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        let font = parse(&self.path, &text)?;
        for c in self.required.chars() {
            if font.glyphs.binary_search_by_key(&c, |g| g.0).is_err() {
                return Err(self.error(
                    None,
                    format!(
                        "no glyph for {c:?} (U+{:04X}), which .require(..) asks for. \
                         Draw it in the font, or stop requiring it",
                        c as u32
                    ),
                ));
            }
        }
        Ok(font)
    }

    /// Write the generated code to `<stem>.rs` and return its path.
    /// Prints `cargo::rerun-if-changed` when writing to `$OUT_DIR`.
    pub fn generate(&self) -> Result<PathBuf, BuildError> {
        let font = self.load()?;
        let from_env = self.out_dir.is_none();
        let dir = match &self.out_dir {
            Some(d) => d.clone(),
            None => PathBuf::from(std::env::var_os("OUT_DIR").ok_or(BuildError::NoOutDir)?),
        };
        if from_env {
            println!("cargo::rerun-if-changed={}", self.path.display());
        }
        let stem = self
            .path
            .file_stem()
            .map_or_else(|| "font".into(), |s| s.to_string_lossy().into_owned());
        let out = dir.join(format!("{stem}.rs"));
        std::fs::write(&out, code(&font, &self.core_path)).map_err(|e| BuildError::Io {
            path: out.clone(),
            message: e.to_string(),
        })?;
        Ok(out)
    }

    fn error(&self, line: Option<usize>, problem: String) -> BuildError {
        font_error(&self.path, line, problem)
    }
}

fn font_error(path: &Path, line: Option<usize>, problem: String) -> BuildError {
    BuildError::BitmapFont {
        path: path.to_owned(),
        line: line.map(|l| l as u32),
        problem,
    }
}

fn parse(path: &Path, text: &str) -> Result<ParsedFont, BuildError> {
    let mut size: Option<(u8, u8, u8)> = None;
    let mut glyphs: Vec<(char, Vec<u8>)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let err = |problem: String| font_error(path, Some(index + 1), problem);
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let mut words = line.split_whitespace();
        let head = words.next().unwrap_or_default();
        let Some((width, height, _)) = size else {
            let nums: Vec<u8> = words.filter_map(|w| w.parse().ok()).collect();
            let &[w, h, a] = nums.as_slice() else {
                return Err(err(
                    "the first line must be `size WIDTH HEIGHT ADVANCE`".into()
                ));
            };
            if head != "size" || !(1..=8).contains(&w) || h == 0 {
                return Err(err(
                    "the first line must be `size WIDTH HEIGHT ADVANCE`, width 1 to 8".into(),
                ));
            }
            size = Some((w, h, a));
            continue;
        };
        let mut letters = head.chars();
        let letter = match (letters.next(), letters.next()) {
            _ if head == "space" => ' ',
            (Some(letter), None) => letter,
            _ => {
                return Err(err(format!(
                    "{head:?} is not a character; write one character, or `space`"
                )));
            }
        };
        let mut rows = Vec::with_capacity(height.into());
        for row in words {
            if row.chars().count() != width.into()
                || row.chars().any(|dot| dot != '.' && dot != '#')
            {
                return Err(err(format!(
                    "glyph {letter:?}: row {row:?} must be {width} of `.` and `#`"
                )));
            }
            rows.push(
                row.chars()
                    .fold(0u8, |bits, dot| bits << 1 | u8::from(dot == '#')),
            );
        }
        if rows.len() != height.into() {
            return Err(err(format!(
                "glyph {letter:?} has {} rows; the face's height is {height}",
                rows.len()
            )));
        }
        if glyphs.iter().any(|g| g.0 == letter) {
            return Err(err(format!("glyph {letter:?} is drawn twice; keep one")));
        }
        glyphs.push((letter, rows));
    }
    let Some((width, height, advance)) = size else {
        return Err(font_error(path, None, "the file has no `size` line".into()));
    };
    glyphs.sort_by_key(|g| g.0);
    Ok(ParsedFont {
        width,
        height,
        advance,
        glyphs,
    })
}

fn code(font: &ParsedFont, core: &str) -> String {
    let mut out = format!(
        "// @generated by silkscreen-build from a bitmap font source.\n\
         pub static FONT: {core}::lcd::LcdFont = {core}::lcd::LcdFont {{\n    \
         width: {},\n    height: {},\n    advance: {},\n    glyphs: &[\n",
        font.width, font.height, font.advance
    );
    for (c, rows) in &font.glyphs {
        let _ = writeln!(out, "        ({c:?}, &{rows:?}),");
    }
    out.push_str("    ],\n};\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO_FONT: &str = crate::LCD_FONT;

    fn parse_str(text: &str) -> Result<ParsedFont, BuildError> {
        parse(Path::new("f.txt"), text)
    }

    #[test]
    fn the_repo_face_covers_printable_ascii_and_degrees() {
        let all: String = (' '..='~').chain(['°']).collect();
        let font = BitmapFont::new(REPO_FONT).require(&all).load().unwrap();
        assert_eq!((font.width, font.height, font.advance), (5, 7, 6));
        assert_eq!(font.glyphs.len(), 96);
    }

    #[test]
    fn a_missing_glyph_is_named_with_the_fix() {
        let e = BitmapFont::new(REPO_FONT)
            .require("Hz€")
            .load()
            .unwrap_err()
            .to_string();
        assert!(e.contains("silk-5x7.txt"), "{e}");
        assert!(e.contains("'€'") && e.contains("U+20AC"), "{e}");
        assert!(e.contains("Draw it in the font"), "{e}");
    }

    #[test]
    fn rows_pack_left_dot_highest() {
        let f = parse_str("size 3 2 4\nA #.. .##\nspace ... ...\n").unwrap();
        assert_eq!(f.glyphs[0], (' ', vec![0, 0]));
        assert_eq!(f.glyphs[1], ('A', vec![0b100, 0b011]));
    }

    #[test]
    fn a_bad_glyph_names_its_line() {
        let e = parse_str("size 3 2 4\n\nA #.. ##\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains(":3") && e.contains("'A'"), "{e}");
        let e = parse_str("size 3 1 4\nA ###\nA ...\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("twice"), "{e}");
        assert!(parse_str("A ###\n").is_err());
    }

    #[test]
    fn generates_a_static_for_the_core_type() {
        let dir = std::env::temp_dir().join(format!("silkscreen-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = BitmapFont::new(REPO_FONT)
            .core_path("silkscreen_core")
            .out_dir(&dir)
            .generate()
            .unwrap();
        assert_eq!(out.file_name().unwrap(), "silk-5x7.rs");
        let code = std::fs::read_to_string(&out).unwrap();
        assert!(code.contains("pub static FONT: silkscreen_core::lcd::LcdFont"));
        assert!(
            code.contains("('A', &[14, 17, 17, 31, 17, 17, 17])"),
            "{code}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
