//! The render cache: each PNG [`Faceplate::generate`](crate::Faceplate::generate)
//! writes is keyed by everything that decides it, and the key is kept beside
//! it (`name.png.key`). When the key matches, the PNG is left as it is:
//! nothing is parsed, rendered or encoded.
//!
//! A build script reruns whenever a build dependency changes. One that
//! needs its engine for the param table would, without this, re-render
//! the whole faceplate on every engine edit.
//!
//! The key covers the renderer (this crate's source and the `Cargo.lock`
//! of the workspace being built), the fonts, the source's bytes and every
//! file it links, the size and the scale.
//!
//! The lockfile is found at run time, from the build script's
//! `CARGO_MANIFEST_DIR` upwards, so the key follows the versions of resvg,
//! tiny-skia and png that this build actually uses, wherever silkscreen
//! itself came from. `SILKSCREEN_BUILD_LOCK`, if set, stands in for the
//! lockfile. With neither, nothing is cached: every PNG is drawn.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use resvg::tiny_skia::Pixmap;
use silkscreen_core::Size;

use crate::error::BuildError;

/// The code that draws: a change to it redraws everything.
const RENDERER: &[&str] = &[include_str!("lib.rs"), include_str!("cache.rs")];

/// What stands for the renderer's dependency versions: the
/// `SILKSCREEN_BUILD_LOCK` variable, else the first `Cargo.lock` from
/// `start` upwards; `None` if there is neither.
fn lock_fingerprint(start: Option<&Path>) -> Option<Vec<u8>> {
    if let Some(v) = std::env::var_os("SILKSCREEN_BUILD_LOCK") {
        return Some(v.as_encoded_bytes().to_vec());
    }
    start?
        .ancestors()
        .find_map(|dir| std::fs::read(dir.join("Cargo.lock")).ok())
}

/// A key under construction. Without a lockfile it can't vouch for the
/// renderer, and nothing is cached.
#[derive(Clone)]
pub(crate) struct Key {
    hash: DefaultHasher,
    cacheable: bool,
}

impl Key {
    /// The renderer and `fonts` (every file in each directory, in order),
    /// and whether system fonts are loaded too.
    pub fn base(fonts: &[PathBuf], system_fonts: bool) -> Self {
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
        let lock = lock_fingerprint(manifest.as_deref());
        Self::base_from(lock.as_deref(), fonts, system_fonts)
    }

    fn base_from(lock: Option<&[u8]>, fonts: &[PathBuf], system_fonts: bool) -> Self {
        let mut h = DefaultHasher::new();
        RENDERER.hash(&mut h);
        lock.hash(&mut h);
        system_fonts.hash(&mut h);
        for dir in fonts {
            let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .collect();
            files.sort();
            for file in files {
                file.hash(&mut h);
                std::fs::read(&file).unwrap_or_default().hash(&mut h);
            }
        }
        Self {
            hash: h,
            cacheable: lock.is_some(),
        }
    }

    /// Add a label, so different outputs from the same source differ.
    pub fn with(mut self, label: &str) -> Self {
        label.hash(&mut self.hash);
        self
    }

    /// Add `bytes` and, for SVG text, each file it links (`href`), as
    /// resolved against `dir`.
    pub fn source(mut self, bytes: &[u8], dir: Option<&Path>) -> Self {
        bytes.hash(&mut self.hash);
        if let (Ok(text), Some(dir)) = (std::str::from_utf8(bytes), dir) {
            for link in links(text) {
                link.hash(&mut self.hash);
                std::fs::read(dir.join(link))
                    .unwrap_or_default()
                    .hash(&mut self.hash);
            }
        }
        self
    }

    /// Add the size and scale an output renders at.
    pub fn at(mut self, size: Size, scale: f32) -> Self {
        (size.w.to_bits(), size.h.to_bits(), scale.to_bits()).hash(&mut self.hash);
        self
    }

    fn hex(&self) -> String {
        format!("{:016x}", self.hash.finish())
    }
}

/// Write `path` from `draw` unless it already holds what `key` describes.
pub(crate) fn cached(
    path: &Path,
    key: &Key,
    draw: impl FnOnce() -> Result<Pixmap, BuildError>,
) -> Result<(), BuildError> {
    let stamp = PathBuf::from(format!("{}.key", path.display()));
    let hex = key.hex();
    if key.cacheable && path.is_file() && std::fs::read_to_string(&stamp).is_ok_and(|k| k == hex) {
        return Ok(());
    }
    let bytes = draw()?.encode_png().map_err(|e| BuildError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    crate::write(path, &bytes)?;
    if key.cacheable {
        crate::write(&stamp, hex.as_bytes())
    } else {
        // A stale stamp must not vouch for this PNG later.
        let _ = std::fs::remove_file(&stamp);
        Ok(())
    }
}

/// The local files SVG `text` links: `href` values that aren't fragments
/// or `data:` URLs.
fn links(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("href=")
        .filter_map(move |(at, m)| {
            let rest = &text[at + m.len()..];
            let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let rest = &rest[1..];
            rest.find(quote).map(|end| &rest[..end])
        })
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("data:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_local_hrefs_only() {
        let svg = r##"<image href="panel/a.png"/><use xlink:href="#knob"/>
            <image xlink:href='b.png'/><image href="data:image/png;base64,AA"/>"##;
        assert_eq!(links(svg).collect::<Vec<_>>(), ["panel/a.png", "b.png"]);
    }

    #[test]
    fn a_matching_key_skips_the_draw() {
        let dir = std::env::temp_dir().join(format!("silkscreen-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x@1x.png");
        let _ = std::fs::remove_file(&path);
        let key = Key::base(&[], false).with("x").at(Size::new(2.0, 2.0), 1.0);
        let draw = || Ok(Pixmap::new(2, 2).unwrap());
        cached(&path, &key, draw).unwrap();
        assert!(path.is_file());
        cached(&path, &key, || panic!("drawn again")).unwrap();
        let mut drawn = false;
        cached(&path, &key.clone().with("changed"), || {
            drawn = true;
            draw()
        })
        .unwrap();
        assert!(drawn, "a new key redraws");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_lockfile_is_found_above_the_manifest() {
        let dir = std::env::temp_dir().join(format!("silkscreen-lock-{}", std::process::id()));
        let crate_dir = dir.join("a/b");
        std::fs::create_dir_all(&crate_dir).unwrap();
        assert_eq!(lock_fingerprint(Some(&crate_dir)), None, "no lockfile yet");
        std::fs::write(dir.join("Cargo.lock"), "v1").unwrap();
        assert_eq!(lock_fingerprint(Some(&crate_dir)), Some(b"v1".to_vec()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_lockfile_every_png_is_drawn() {
        let dir = std::env::temp_dir().join(format!("silkscreen-nolock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x@1x.png");
        let key = Key::base_from(None, &[], false).with("x");
        let mut draws = 0;
        for _ in 0..2 {
            cached(&path, &key, || {
                draws += 1;
                Ok(Pixmap::new(2, 2).unwrap())
            })
            .unwrap();
        }
        assert_eq!(draws, 2);
        assert!(!dir.join("x@1x.png.key").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
