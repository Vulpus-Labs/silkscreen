//! Build-time SVG faceplate pipeline ([design/faceplates.md]).
//!
//! Call from `build.rs`:
//!
//! ```no_run
//! silkscreen_build::Faceplate::new("gain.svg")
//!     .scales(&[1.0, 1.5, 2.0])
//!     .build();
//! ```
//!
//! This reads `gain.svg`, extracts the controls layer, renders the rest to
//! `gain@{scale}x.png` in `OUT_DIR`, and writes `gain.rs` there. Pull the
//! generated code in with:
//!
//! ```ignore
//! mod faceplate {
//!     include!(concat!(env!("OUT_DIR"), "/gain.rs"));
//! }
//! ```
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

mod bitmap_font;
mod build_id;
mod cache;
mod codegen;
mod error;
mod extract;
mod factory;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use resvg::tiny_skia::{Pixmap, Transform};
use silkscreen_core::{ControlKind, FaderGeometry, Rect, Size};

pub use bitmap_font::{BitmapFont, ParsedFont};
pub use build_id::stamp_build_id;
pub use error::BuildError;

/// The folder holding the editor's runtime face, Jost Medium, for
/// [`Faceplate::font_dir`]: faceplate lettering that matches tooltips and
/// readouts.
pub const TEXT_FONT_DIR: &str = silkscreen_text::FONT_DIR;

/// silkscreen's 5x7 LCD face (printable ASCII and the degree sign), for
/// [`BitmapFont::new`].
pub const LCD_FONT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/silk-5x7.txt");
pub use factory::{FactoryBank, FactoryEntry};

/// One control found in the SVG's controls layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub name: String,
    pub kind: ControlKind,
    /// Absolute bounding box in logical units.
    pub rect: Rect,
    /// The skin named by the marker's `data-skin`, if any.
    pub skin: Option<String>,
    /// The page it's on, `(group, page)` ([design/faceplates.md]).
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    pub page: Option<(String, u16)>,
    /// The marker's SVG id and line, for messages.
    pub id: String,
    pub line: u32,
}

/// A skin as registered with [`Faceplate::skin`].
#[derive(Clone, Debug)]
struct SkinSource {
    name: String,
    frames: Vec<PathBuf>,
    sweep_deg: Option<f32>,
    cycle_deg: Option<f32>,
}

/// A skin in use by controls of `kind`, all of logical `size`.
#[derive(Clone, Debug)]
struct UsedSkin<'a> {
    source: &'a SkinSource,
    kind: ControlKind,
    size: Size,
}

impl UsedSkin<'_> {
    /// The logical size frame `index` renders at: the control's, except a
    /// fader's or selector's cap (its last frame; a four-frame fader's
    /// last two, lit and dark), at the cap's size.
    fn frame_size(&self, index: usize) -> Size {
        let frames = self.source.frames.len();
        let cap = match self.kind {
            ControlKind::Fader if frames == 4 => index >= 2,
            ControlKind::Fader | ControlKind::Selector => index + 1 == frames,
            _ => false,
        };
        if cap {
            let rect = Rect::new(0.0, 0.0, self.size.w, self.size.h);
            FaderGeometry::new(rect).cap(0.0).size()
        } else {
            self.size
        }
    }
}

/// The result of analysing a faceplate SVG, before anything is written.
#[derive(Clone, Debug)]
pub struct Analysis {
    /// Logical size: the SVG's `width`/`height`.
    pub size: Size,
    /// Controls in document order.
    pub controls: Vec<Control>,
    /// The SVG source with the controls layer and every page's art
    /// removed.
    pub faceplate_svg: String,
    /// Page groups, in order of their first page's art.
    pub pages: Vec<PageGroupInfo>,
}

/// A page group found in the SVG ([design/faceplates.md]).
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
#[derive(Clone, Debug)]
pub struct PageGroupInfo {
    pub name: String,
    /// The union of its pages' art bounds, logical.
    pub region: Rect,
    /// Page `n`'s faceplate: the SVG with the controls layer and every
    /// other page's art removed.
    pub page_svgs: Vec<String>,
}

/// What [`Faceplate::generate`] wrote.
#[derive(Clone, Debug)]
pub struct Generated {
    pub analysis: Analysis,
    /// `(scale, path)` of each PNG.
    pub pngs: Vec<(f32, PathBuf)>,
    /// The generated Rust module.
    pub code: PathBuf,
}

/// Builder for one faceplate.
#[derive(Clone, Debug)]
pub struct Faceplate {
    svg: PathBuf,
    scales: Vec<f32>,
    font_dirs: Vec<PathBuf>,
    system_fonts: bool,
    core_path: String,
    out_dir: Option<PathBuf>,
    skins: Vec<SkinSource>,
    fetch: Option<String>,
}

impl Faceplate {
    pub fn new(svg: impl Into<PathBuf>) -> Self {
        Self {
            svg: svg.into(),
            scales: vec![1.0, 1.5, 2.0],
            font_dirs: Vec::new(),
            system_fonts: false,
            core_path: "::silkscreen::core".to_owned(),
            out_dir: None,
            skins: Vec::new(),
            fetch: None,
        }
    }

    /// Register a skin: sprite frames for controls whose marker says
    /// `data-skin="name"` ([design/faceplates.md]). Each frame is an SVG, rendered
    /// at the size of the controls that use it, at every scale. A knob
    /// takes two frames (a static base, then a top that rotates); a
    /// toggle, button or radio button takes two (off, then on).
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    pub fn skin(mut self, name: impl Into<String>, frames: &[impl AsRef<Path>]) -> Self {
        self.skins.push(SkinSource {
            name: name.into(),
            frames: frames.iter().map(|f| f.as_ref().to_path_buf()).collect(),
            sweep_deg: None,
            cycle_deg: None,
        });
        self
    }

    /// Make a knob skin registered with [`skin`](Self::skin) cycled: its
    /// first frame is the pointer alone (drawn at 12 o'clock), and the rest
    /// are the body turned evenly across one `period_deg`, the angle after
    /// which it looks the same (e.g. 360 / knurl ridges). The pointer turns
    /// exactly; the body steps by `period_deg / (frames − 1)`.
    pub fn skin_cycle(mut self, name: &str, period_deg: f32) -> Self {
        match self.skins.iter_mut().find(|s| s.name == name) {
            Some(s) => s.cycle_deg = Some(period_deg),
            None => panic!(
                "skin_cycle(\"{name}\", ..): register the skin with .skin(\"{name}\", ..) first"
            ),
        }
        self
    }

    /// For a knob skin registered with [`skin`](Self::skin): the top's
    /// total rotation from minimum to maximum, centred on its drawn
    /// orientation. Default 270°.
    pub fn skin_sweep(mut self, name: &str, degrees: f32) -> Self {
        match self.skins.iter_mut().find(|s| s.name == name) {
            Some(s) => s.sweep_deg = Some(degrees),
            None => panic!(
                "skin_sweep(\"{name}\", ..): register the skin with .skin(\"{name}\", ..) first"
            ),
        }
        self
    }

    /// Scales to pre-render. Default `1.0, 1.5, 2.0`.
    pub fn scales(mut self, scales: &[f32]) -> Self {
        self.scales = scales.to_vec();
        self
    }

    /// A directory of `.ttf`/`.otf`/`.ttc` fonts for `<text>` elements. Only
    /// fonts loaded this way (or system fonts, if enabled) are used, so
    /// renders are identical on every machine.
    pub fn font_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.font_dirs.push(dir.into());
        self
    }

    /// Also use system fonts. Off by default: output then depends on the
    /// build machine.
    pub fn system_fonts(mut self, enabled: bool) -> Self {
        self.system_fonts = enabled;
        self
    }

    /// Path through which generated code names `silkscreen-core` types.
    /// Default `::silkscreen::core`.
    pub fn core_path(mut self, path: impl Into<String>) -> Self {
        self.core_path = path.into();
        self
    }

    /// Fetch mode ([design/hosts.md]): name each PNG by a URL under `prefix`
    /// (relative to the page) instead of embedding it, for the web host
    /// to fetch at run time. A manifest, `<stem>.assets` beside the code,
    /// lists each URL and the file to serve there.
    ///
    /// In a build script this is on by default when the crate's `fetch`
    /// feature is, with the prefix `assets/<crate name>`.
    ///
    /// [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md
    pub fn fetch(mut self, prefix: impl Into<String>) -> Self {
        self.fetch = Some(prefix.into());
        self
    }

    /// Write outputs here instead of `$OUT_DIR`.
    pub fn out_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.out_dir = Some(dir.into());
        self
    }

    /// [`generate`](Self::generate), panicking with the error's message on
    /// failure. Meant for `build.rs`, where a panic is how errors surface.
    pub fn build(&self) -> Generated {
        match self.generate() {
            Ok(g) => g,
            Err(e) => panic!("\n\nsilkscreen-build: {e}\n\n"),
        }
    }

    /// Analyse the SVG without writing anything.
    pub fn analyze(&self) -> Result<Analysis, BuildError> {
        let text = std::fs::read_to_string(&self.svg).map_err(|e| BuildError::Io {
            path: self.svg.clone(),
            message: e.to_string(),
        })?;
        self.analyze_str(&text)
    }

    /// Analyse SVG source text. `self`'s path is used only in messages.
    pub fn analyze_str(&self, text: &str) -> Result<Analysis, BuildError> {
        let layer = extract::controls_layer(&self.svg, text)?;
        let tree = self.parse(text)?;
        let size = Size::new(tree.size().width(), tree.size().height());
        let controls = extract::control_rects(&self.svg, &tree, &layer.markers)?;
        self.used_skins(&controls)?;
        let all_pages: Vec<_> = layer.pages.iter().map(|p| p.range.clone()).collect();
        let faceplate_svg = without(text, &[&[layer.range.clone()][..], &all_pages].concat());
        let mut pages: Vec<PageGroupInfo> = Vec::new();
        for art in &layer.pages {
            let b = tree
                .node_by_id(&art.id)
                .and_then(|n| n.abs_bounding_box().to_non_zero_rect())
                .ok_or_else(|| BuildError::PageGroup {
                    path: self.svg.clone(),
                    group: art.group.clone(),
                    problem: format!(
                        "has page art id=\"{}\" with nothing in it. Draw the page's art in the group",
                        art.id
                    ),
                })?;
            let bounds = Rect::new(b.x(), b.y(), b.width(), b.height());
            if let Some(group) = pages.iter_mut().find(|g| g.name == art.group) {
                group.region = union(group.region, bounds);
            } else {
                pages.push(PageGroupInfo {
                    name: art.group.clone(),
                    region: bounds,
                    page_svgs: Vec::new(),
                });
            }
        }
        for group in &mut pages {
            let count = layer.pages.iter().filter(|p| p.group == group.name).count();
            for n in 0..count {
                let others: Vec<_> = layer
                    .pages
                    .iter()
                    .filter(|p| !(p.group == group.name && usize::from(p.page) == n))
                    .map(|p| p.range.clone())
                    .collect();
                group.page_svgs.push(without(
                    text,
                    &[&[layer.range.clone()][..], &others].concat(),
                ));
            }
            let r = group.region;
            if let Some(c) = controls.iter().find(|c| {
                c.page.as_ref().is_some_and(|(g, _)| *g == group.name)
                    && !(c.rect.x >= r.x - 0.5
                        && c.rect.y >= r.y - 0.5
                        && c.rect.right() <= r.right() + 0.5
                        && c.rect.bottom() <= r.bottom() + 0.5)
            }) {
                return Err(BuildError::PageGroup {
                    path: self.svg.clone(),
                    group: group.name.clone(),
                    problem: format!(
                        "spans {r:?}, but control \"{}\" (line {}) on one of its pages lies \
                         outside it. Move the control in, or widen the pages' art (a background \
                         rect does it)",
                        c.id, c.line
                    ),
                });
            }
        }
        Ok(Analysis {
            size,
            controls,
            faceplate_svg,
            pages,
        })
    }

    /// Analyse, render every scale, and write PNGs plus the Rust module.
    /// Prints `cargo::rerun-if-changed` when writing to `$OUT_DIR`.
    pub fn generate(&self) -> Result<Generated, BuildError> {
        let (out_dir, in_build_script) = match &self.out_dir {
            Some(d) => (d.clone(), false),
            None => (
                std::env::var_os("OUT_DIR")
                    .map(PathBuf::from)
                    .ok_or(BuildError::NoOutDir)?,
                true,
            ),
        };
        if in_build_script {
            println!("cargo::rerun-if-changed={}", self.svg.display());
            for dir in &self.font_dirs {
                println!("cargo::rerun-if-changed={}", dir.display());
            }
        }

        let refs = Refs::new(in_build_script, self.fetch.clone());

        let analysis = self.analyze()?;
        let stem = self.stem()?;
        // Each PNG is redrawn only when what decides it has changed
        // (`cache.rs`); the trees are parsed only if one is.
        let base = cache::Key::base(&self.font_dirs, self.system_fonts);
        let dir = self.svg.parent();
        let face_key = base
            .clone()
            .with("faceplate")
            .source(analysis.faceplate_svg.as_bytes(), dir);
        let faceplate_tree = std::cell::OnceCell::new();

        let mut pngs = Vec::new();
        for &scale in &self.scales {
            let path = out_dir.join(format!("{stem}@{scale}x.png"));
            cache::cached(&path, &face_key.clone().at(analysis.size, scale), || {
                let tree = once(&faceplate_tree, || self.parse(&analysis.faceplate_svg))?;
                self.render(tree, analysis.size, scale)
            })?;
            pngs.push((scale, path));
        }

        let refer = |path: &Path| refs.refer(path);
        let pages = self.render_pages(&analysis, &out_dir, &stem, &refs, &base)?;
        let skins = self.render_skins(&analysis, &out_dir, &stem, &refs, &base, in_build_script)?;

        let code_path = out_dir.join(format!("{stem}.rs"));
        let png_refs: Vec<(f32, codegen::PngRef)> = pngs
            .iter()
            .map(|(scale, path)| (*scale, refer(path)))
            .collect();
        let code = codegen::generate(&codegen::Input {
            source: &self.svg,
            core: &self.core_path,
            size: analysis.size,
            controls: &analysis.controls,
            pngs: &png_refs,
            skins: &skins,
            pages: &pages,
        });
        write(&code_path, code.as_bytes())?;
        refs.write_manifest(&out_dir.join(format!("{stem}.assets")))?;

        Ok(Generated {
            analysis,
            pngs,
            code: code_path,
        })
    }

    /// Render each used skin's frames at every scale.
    fn render_skins(
        &self,
        analysis: &Analysis,
        out_dir: &Path,
        stem: &str,
        refs: &Refs,
        base: &cache::Key,
        in_build_script: bool,
    ) -> Result<Vec<codegen::SkinCode>, BuildError> {
        let mut skins = Vec::new();
        for used in self.used_skins(&analysis.controls)? {
            let mut frames = Vec::new();
            for (i, frame) in used.source.frames.iter().enumerate() {
                if in_build_script {
                    println!("cargo::rerun-if-changed={}", frame.display());
                }
                let bytes = std::fs::read(frame).map_err(|e| BuildError::Io {
                    path: frame.clone(),
                    message: e.to_string(),
                })?;
                let key = base.clone().with("skin").source(&bytes, self.svg.parent());
                let source = std::cell::OnceCell::new();
                let size = used.frame_size(i);
                let mut pngs = Vec::new();
                for &scale in &self.scales {
                    let path =
                        out_dir.join(format!("{stem}.skin.{}.{i}@{scale}x.png", used.source.name));
                    cache::cached(&path, &key.clone().at(size, scale), || {
                        match once(&source, || self.frame_source(frame))? {
                            FrameSource::Svg(tree) => render_sprite(frame, tree, size, scale),
                            FrameSource::Png(png) => resample_sprite(frame, png, size, scale),
                        }
                    })?;
                    pngs.push((scale, refs.refer(&path)));
                }
                frames.push((size, pngs));
            }
            skins.push(codegen::SkinCode {
                name: used.source.name.clone(),
                frames,
                sweep_deg: used.source.sweep_deg,
                cycle_deg: used.source.cycle_deg,
            });
        }
        Ok(skins)
    }

    /// Render each page group's pages, each the whole faceplate with that
    /// page's art, at every scale.
    fn render_pages(
        &self,
        analysis: &Analysis,
        out_dir: &Path,
        stem: &str,
        refs: &Refs,
        base: &cache::Key,
    ) -> Result<Vec<codegen::PageGroupCode>, BuildError> {
        let mut page_code = Vec::new();
        for group in &analysis.pages {
            let mut pages: Vec<Vec<(f32, codegen::PngRef)>> = Vec::new();
            // Each page's rasters so far, to share the first of any that match.
            let mut rendered: Vec<Vec<Vec<u8>>> = Vec::new();
            for (n, svg) in group.page_svgs.iter().enumerate() {
                let key = base
                    .clone()
                    .with("page")
                    .source(svg.as_bytes(), self.svg.parent());
                let tree = std::cell::OnceCell::new();
                let mut paths = Vec::new();
                let mut bytes = Vec::new();
                for &scale in &self.scales {
                    let path = out_dir.join(format!("{stem}.page.{}.{n}@{scale}x.png", group.name));
                    cache::cached(&path, &key.clone().at(analysis.size, scale), || {
                        let tree = once(&tree, || self.parse(svg))?;
                        self.render(tree, analysis.size, scale)
                    })?;
                    bytes.push(std::fs::read(&path).map_err(|e| BuildError::Io {
                        path: path.clone(),
                        message: e.to_string(),
                    })?);
                    paths.push((scale, path));
                }
                // Pages that look alike (one per oscillator, say) share one raster.
                let pngs = match rendered.iter().position(|earlier| *earlier == bytes) {
                    Some(same) => pages[same].clone(),
                    None => paths
                        .iter()
                        .map(|(scale, path)| (*scale, refs.refer(path)))
                        .collect(),
                };
                rendered.push(bytes);
                pages.push(pngs);
            }
            page_code.push(codegen::PageGroupCode {
                name: group.name.clone(),
                region: group.region,
                pages,
            });
        }
        Ok(page_code)
    }

    /// The registered skins the controls use, in registration order, each
    /// with the size its frames render at. Every control's skin must be
    /// registered, have the right number of frames for its kind, and be
    /// used at one size only.
    fn used_skins(&self, controls: &[Control]) -> Result<Vec<UsedSkin<'_>>, BuildError> {
        let mut used: Vec<(UsedSkin<'_>, &Control)> = Vec::new();
        for skin in &self.skins {
            if !extract::is_snake_ident_pub(&skin.name) {
                return Err(BuildError::BadSkinName {
                    name: skin.name.clone(),
                });
            }
        }
        for c in controls {
            let Some(name) = &c.skin else { continue };
            let Some(source) = self.skins.iter().find(|s| &s.name == name) else {
                return Err(BuildError::UnknownSkin {
                    path: self.svg.clone(),
                    line: c.line,
                    id: c.id.clone(),
                    skin: name.clone(),
                    known: self.skins.iter().map(|s| s.name.clone()).collect(),
                });
            };
            let expected = match c.kind {
                // Two frames (base, rotating top), or a filmstrip of three
                // or more, one per angle.
                // Cycled: a pointer and at least one body frame.
                ControlKind::Knob if source.cycle_deg.is_some() && source.frames.len() >= 2 => {
                    source.frames.len()
                }
                ControlKind::Knob if source.frames.len() >= 3 => source.frames.len(),
                // A tab unselected and selected, or each with its lamp off
                // and lit; or a fader's tracks, then its cap lit and dark
                // (dark when the fader is dimmed, which it then draws
                // itself).
                ControlKind::Tab { .. } | ControlKind::Fader if source.frames.len() == 4 => 4,
                // A three-way switch (down, middle, up); or a fader's empty
                // and filled tracks and its cap.
                ControlKind::Toggle | ControlKind::Fader if source.frames.len() == 3 => 3,
                ControlKind::Knob
                | ControlKind::Tab { .. }
                | ControlKind::Toggle
                | ControlKind::Button
                | ControlKind::Radio { .. } => 2,
                // The cap alone over a vector slot; or a selector's track
                // and cap.
                ControlKind::Selector if source.frames.len() == 2 => 2,
                ControlKind::Fader | ControlKind::Selector => 1,
                // Kinds added later.
                _ => {
                    return Err(BuildError::SkinUnsupported {
                        path: self.svg.clone(),
                        line: c.line,
                        id: c.id.clone(),
                    });
                }
            };
            if source.frames.len() != expected {
                return Err(BuildError::SkinFrames {
                    path: self.svg.clone(),
                    line: c.line,
                    id: c.id.clone(),
                    skin: name.clone(),
                    expected,
                    got: source.frames.len(),
                });
            }
            let size = c.rect.size();
            match used.iter().find(|(u, _)| u.source.name == *name) {
                Some((u, first)) => {
                    if (u.size.w - size.w).abs() > 0.5 || (u.size.h - size.h).abs() > 0.5 {
                        return Err(BuildError::SkinSizes {
                            path: self.svg.clone(),
                            skin: name.clone(),
                            first: (first.id.clone(), u.size),
                            second: (c.id.clone(), size),
                        });
                    }
                }
                None => used.push((
                    UsedSkin {
                        source,
                        kind: c.kind,
                        size,
                    },
                    c,
                )),
            }
        }
        Ok(used.into_iter().map(|(u, _)| u).collect())
    }

    /// A frame file: SVG to render, or PNG to resample.
    fn frame_source(&self, path: &Path) -> Result<FrameSource, BuildError> {
        let is_png = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        if is_png {
            let png = Pixmap::load_png(path).map_err(|e| BuildError::Io {
                path: path.to_path_buf(),
                message: format!("not a readable PNG: {e}"),
            })?;
            Ok(FrameSource::Png(png))
        } else {
            self.parse_file(path).map(|t| FrameSource::Svg(Box::new(t)))
        }
    }

    fn parse_file(&self, path: &Path) -> Result<usvg::Tree, BuildError> {
        let text = std::fs::read_to_string(path).map_err(|e| BuildError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        self.parse(&text).map_err(|e| match e {
            BuildError::Svg { message, .. } => BuildError::Svg {
                path: path.to_path_buf(),
                message,
            },
            BuildError::MissingFont { families, .. } => BuildError::MissingFont {
                path: path.to_path_buf(),
                families,
            },
            other => other,
        })
    }

    fn stem(&self) -> Result<String, BuildError> {
        self.svg
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| BuildError::Io {
                path: self.svg.clone(),
                message: "path has no file name".to_owned(),
            })
    }

    fn options(
        &self,
        missing_fonts: Arc<Mutex<Vec<String>>>,
    ) -> Result<usvg::Options<'static>, BuildError> {
        let mut opt = usvg::Options::default();
        if let Some(dir) = self.svg.parent() {
            opt.resources_dir = Some(dir.to_path_buf());
        }
        let db = opt.fontdb_mut();
        if self.system_fonts {
            db.load_system_fonts();
        }
        for dir in &self.font_dirs {
            load_font_dir(db, dir)?;
        }
        opt.font_resolver.select_font = Box::new(move |font, db| {
            let families: Vec<usvg::fontdb::Family> =
                font.families().iter().map(to_fontdb_family).collect();
            let query = usvg::fontdb::Query {
                families: &families,
                weight: usvg::fontdb::Weight(font.weight()),
                stretch: to_fontdb_stretch(font.stretch()),
                style: to_fontdb_style(font.style()),
            };
            let id = db.query(&query);
            if id.is_none() {
                let names: Vec<String> = font.families().iter().map(ToString::to_string).collect();
                missing_fonts.lock().unwrap().push(names.join(", "));
            }
            id
        });
        Ok(opt)
    }

    fn parse(&self, text: &str) -> Result<usvg::Tree, BuildError> {
        let missing = Arc::new(Mutex::new(Vec::new()));
        let opt = self.options(missing.clone())?;
        let tree = usvg::Tree::from_str(text, &opt).map_err(|e| BuildError::Svg {
            path: self.svg.clone(),
            message: e.to_string(),
        })?;
        let mut missing = missing.lock().unwrap().clone();
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(BuildError::MissingFont {
                path: self.svg.clone(),
                families: missing,
            });
        }
        Ok(tree)
    }

    fn render(&self, tree: &usvg::Tree, size: Size, scale: f32) -> Result<Pixmap, BuildError> {
        let (w, h) = size.to_physical(scale);
        let mut pixmap = Pixmap::new(w, h).ok_or(BuildError::ZeroSizeFaceplate {
            path: self.svg.clone(),
        })?;
        let transform = Transform::from_scale(w as f32 / size.w, h as f32 / size.h);
        resvg::render(tree, transform, &mut pixmap.as_mut());
        let translucent = pixmap.pixels().iter().filter(|p| p.alpha() != 255).count();
        if translucent > 0 {
            return Err(BuildError::NotOpaque {
                path: self.svg.clone(),
                scale,
                pixels: translucent,
            });
        }
        Ok(pixmap)
    }
}

/// `cell`'s value, made by `make` the first time it's asked for.
fn once<T>(
    cell: &std::cell::OnceCell<T>,
    make: impl FnOnce() -> Result<T, BuildError>,
) -> Result<&T, BuildError> {
    if let Some(value) = cell.get() {
        return Ok(value);
    }
    let value = make()?;
    Ok(cell.get_or_init(|| value))
}

enum FrameSource {
    Svg(Box<usvg::Tree>),
    Png(Pixmap),
}

/// A pre-rendered sprite frame resampled to `size` × `scale` (bicubic),
/// stretched to fill. Render PNG frames larger than their largest use, so
/// this only ever shrinks them.
fn resample_sprite(
    path: &Path,
    png: &Pixmap,
    size: Size,
    scale: f32,
) -> Result<Pixmap, BuildError> {
    let (w, h) = size.to_physical(scale);
    let mut out = Pixmap::new(w, h).ok_or_else(|| BuildError::Io {
        path: path.to_path_buf(),
        message: "skin renders at zero size".to_owned(),
    })?;
    let paint = resvg::tiny_skia::PixmapPaint {
        quality: resvg::tiny_skia::FilterQuality::Bicubic,
        ..resvg::tiny_skia::PixmapPaint::default()
    };
    // Bicubic reads only 4×4 source pixels, so shrinking far in one step
    // skips detail and aliases fine highlights. Halve (averaging 2×2) until
    // within 2× of the target first.
    let mut src = std::borrow::Cow::Borrowed(png);
    while src.width() >= 2 * w && src.height() >= 2 * h {
        src = std::borrow::Cow::Owned(halve(&src));
    }
    let transform = Transform::from_scale(
        w as f32 / src.width() as f32,
        h as f32 / src.height() as f32,
    );
    out.draw_pixmap(0, 0, src.as_ref().as_ref(), &paint, transform, None);
    Ok(out)
}

/// `png` at half its size (rounded down), each pixel the mean of a 2×2
/// block. Premultiplied, so transparent edges don't darken.
fn halve(png: &Pixmap) -> Pixmap {
    let (width, height) = (png.width() / 2, png.height() / 2);
    let mut out = Pixmap::new(width, height).expect("halved only while at least 2x2");
    let src = png.pixels();
    let stride = png.width() as usize;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let top = 2 * y * stride + 2 * x;
            let block = [
                src[top],
                src[top + 1],
                src[top + stride],
                src[top + stride + 1],
            ];
            let mean = |channel: fn(&resvg::tiny_skia::PremultipliedColorU8) -> u8| {
                let sum: u16 = block.iter().map(|p| u16::from(channel(p))).sum();
                ((sum + 2) / 4) as u8
            };
            out.pixels_mut()[y * width as usize + x] =
                resvg::tiny_skia::PremultipliedColorU8::from_rgba(
                    mean(|p| p.red()),
                    mean(|p| p.green()),
                    mean(|p| p.blue()),
                    mean(|p| p.alpha()),
                )
                .expect("the mean of premultiplied pixels is premultiplied");
        }
    }
    out
}

/// A sprite frame at `size` × `scale`, its viewBox stretched to fill.
/// Sprites may be transparent.
fn render_sprite(
    path: &Path,
    tree: &usvg::Tree,
    size: Size,
    scale: f32,
) -> Result<Pixmap, BuildError> {
    let (w, h) = size.to_physical(scale);
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| BuildError::Io {
        path: path.to_path_buf(),
        message: "skin renders at zero size".to_owned(),
    })?;
    let svg = tree.size();
    let transform = Transform::from_scale(w as f32 / svg.width(), h as f32 / svg.height());
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), BuildError> {
    // Leave unchanged files alone so their mtimes don't trigger rebuilds.
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(|e| BuildError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn load_font_dir(db: &mut usvg::fontdb::Database, dir: &Path) -> Result<(), BuildError> {
    let io = |e: std::io::Error| BuildError::Io {
        path: dir.to_path_buf(),
        message: e.to_string(),
    };
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(io)?;
    // Load in a stable order so font matching is deterministic.
    entries.sort();
    for path in entries {
        let is_font = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"));
        if is_font {
            let data = std::fs::read(&path).map_err(io)?;
            db.load_font_data(data);
        }
    }
    Ok(())
}

fn to_fontdb_family(f: &usvg::FontFamily) -> usvg::fontdb::Family<'_> {
    use usvg::fontdb::Family;
    match f {
        usvg::FontFamily::Serif => Family::Serif,
        usvg::FontFamily::SansSerif => Family::SansSerif,
        usvg::FontFamily::Cursive => Family::Cursive,
        usvg::FontFamily::Fantasy => Family::Fantasy,
        usvg::FontFamily::Monospace => Family::Monospace,
        usvg::FontFamily::Named(s) => Family::Name(s),
    }
}

fn to_fontdb_stretch(s: usvg::FontStretch) -> usvg::fontdb::Stretch {
    use usvg::fontdb::Stretch;
    match s {
        usvg::FontStretch::UltraCondensed => Stretch::UltraCondensed,
        usvg::FontStretch::ExtraCondensed => Stretch::ExtraCondensed,
        usvg::FontStretch::Condensed => Stretch::Condensed,
        usvg::FontStretch::SemiCondensed => Stretch::SemiCondensed,
        usvg::FontStretch::Normal => Stretch::Normal,
        usvg::FontStretch::SemiExpanded => Stretch::SemiExpanded,
        usvg::FontStretch::Expanded => Stretch::Expanded,
        usvg::FontStretch::ExtraExpanded => Stretch::ExtraExpanded,
        usvg::FontStretch::UltraExpanded => Stretch::UltraExpanded,
    }
}

fn to_fontdb_style(s: usvg::FontStyle) -> usvg::fontdb::Style {
    use usvg::fontdb::Style;
    match s {
        usvg::FontStyle::Normal => Style::Normal,
        usvg::FontStyle::Italic => Style::Italic,
        usvg::FontStyle::Oblique => Style::Oblique,
    }
}

/// How generated code names each PNG, and in fetch mode, the URLs given
/// out so far.
struct Refs {
    in_build_script: bool,
    /// The URL prefix, in fetch mode.
    fetch: Option<String>,
    /// `(url, path)` of each PNG named by URL.
    files: std::cell::RefCell<Vec<(String, PathBuf)>>,
}

impl Refs {
    /// `fetch` is the builder's prefix; in a build script, the crate's
    /// `fetch` feature turns fetch mode on with `assets/<crate name>`.
    fn new(in_build_script: bool, fetch: Option<String>) -> Self {
        let fetch = fetch.or_else(|| {
            let on = in_build_script && std::env::var_os("CARGO_FEATURE_FETCH").is_some();
            let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
            on.then(|| format!("assets/{crate_name}"))
        });
        Self {
            in_build_script,
            fetch,
            files: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// In fetch mode, write each URL and its file, a line each.
    fn write_manifest(&self, path: &Path) -> Result<(), BuildError> {
        if self.fetch.is_none() {
            return Ok(());
        }
        let mut manifest = String::new();
        for (url, file) in self.files.borrow().iter() {
            manifest.push_str(url);
            manifest.push('\t');
            manifest.push_str(&file.display().to_string());
            manifest.push('\n');
        }
        write(path, manifest.as_bytes())
    }

    fn refer(&self, path: &Path) -> codegen::PngRef {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        if let Some(prefix) = &self.fetch {
            let url = format!("{prefix}/{file}");
            self.files
                .borrow_mut()
                .push((url.clone(), path.to_path_buf()));
            codegen::PngRef::Url(url)
        } else if self.in_build_script {
            codegen::PngRef::OutDir(file)
        } else {
            codegen::PngRef::Absolute(path.display().to_string())
        }
    }
}

/// `text` with every byte range in `ranges` cut out.
fn without(text: &str, ranges: &[std::ops::Range<usize>]) -> String {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for r in sorted {
        if r.start >= at {
            out.push_str(&text[at..r.start]);
            at = r.end;
        } else {
            at = at.max(r.end);
        }
    }
    out.push_str(&text[at..]);
    out
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_each_2x2_block() {
        let mut png = Pixmap::new(4, 2).unwrap();
        png.fill(resvg::tiny_skia::Color::BLACK);
        // One white pixel in the left block, the right block transparent.
        png.pixels_mut()[0] =
            resvg::tiny_skia::PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
        for i in [2, 3, 6, 7] {
            png.pixels_mut()[i] = resvg::tiny_skia::PremultipliedColorU8::TRANSPARENT;
        }
        let half = halve(&png);
        assert_eq!((half.width(), half.height()), (2, 1));
        let [left, right] = [half.pixels()[0], half.pixels()[1]];
        assert_eq!((left.red(), left.alpha()), (64, 255));
        assert_eq!(right.alpha(), 0);
    }

    #[test]
    fn large_shrinks_keep_thin_lines() {
        // A 1px white line every 10px, shrunk 10x: one-pass bicubic can
        // miss it entirely; the halved result keeps every line's light.
        let mut png = Pixmap::new(200, 20).unwrap();
        png.fill(resvg::tiny_skia::Color::BLACK);
        for y in 0..20 {
            for x in (5..200).step_by(10) {
                png.pixels_mut()[y * 200 + x] =
                    resvg::tiny_skia::PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
            }
        }
        let out = resample_sprite(Path::new("t.png"), &png, Size::new(20.0, 2.0), 1.0).unwrap();
        let reds: Vec<u8> = out.pixels()[..20].iter().map(|p| p.red()).collect();
        let (lo, hi) = (reds.iter().min().unwrap(), reds.iter().max().unwrap());
        assert!(*lo > 10 && hi - lo < 20, "uneven: {reds:?}");
    }
}
