use std::fmt;
use std::path::PathBuf;

/// Everything that can go wrong turning an SVG into a faceplate. Each message
/// names the file, the element (with line number where there is one) and the
/// fix.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
    Io {
        path: PathBuf,
        message: String,
    },
    /// A factory preset ([design/presets.md]) that can't ship: `problem` says
    /// what and how to fix it.
    ///
    /// [design/presets.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/presets.md
    FactoryPreset {
        path: PathBuf,
        problem: String,
    },
    /// A bitmap font ([design/faceplates.md]) that can't be used: `problem` says
    /// what and how to fix it.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    BitmapFont {
        path: PathBuf,
        line: Option<u32>,
        problem: String,
    },
    NoOutDir,
    Xml {
        path: PathBuf,
        message: String,
    },
    Svg {
        path: PathBuf,
        message: String,
    },
    NoControlsLayer {
        path: PathBuf,
    },
    /// A page group ([design/faceplates.md]) whose tabs, pages or controls don't
    /// agree. `problem` says what and how to fix it.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    PageGroup {
        path: PathBuf,
        group: String,
        problem: String,
    },
    MultipleControlsLayers {
        path: PathBuf,
        lines: Vec<u32>,
    },
    ControlsLayerHidden {
        path: PathBuf,
        line: u32,
    },
    MarkerWithoutId {
        path: PathBuf,
        line: u32,
        element: String,
    },
    BadControlId {
        path: PathBuf,
        line: u32,
        id: String,
        reason: BadIdReason,
    },
    DuplicateName {
        path: PathBuf,
        name: String,
        lines: (u32, u32),
    },
    /// A control names a skin that wasn't registered.
    UnknownSkin {
        path: PathBuf,
        line: u32,
        id: String,
        skin: String,
        known: Vec<String>,
    },
    /// A skin has the wrong number of frames for a control using it.
    SkinFrames {
        path: PathBuf,
        line: u32,
        id: String,
        skin: String,
        expected: usize,
        got: usize,
    },
    /// A control kind that doesn't take skins.
    SkinUnsupported {
        path: PathBuf,
        line: u32,
        id: String,
    },
    /// One skin used by controls of different sizes.
    SkinSizes {
        path: PathBuf,
        skin: String,
        first: (String, silkscreen_core::Size),
        second: (String, silkscreen_core::Size),
    },
    /// A skin name that can't be a Rust identifier.
    BadSkinName {
        name: String,
    },
    /// A radio group's positions aren't `0..n`.
    RadioGap {
        path: PathBuf,
        group: String,
        missing: u16,
        count: usize,
    },
    NoGeometry {
        path: PathBuf,
        id: String,
    },
    ZeroSize {
        path: PathBuf,
        id: String,
    },
    ZeroSizeFaceplate {
        path: PathBuf,
    },
    MissingFont {
        path: PathBuf,
        families: Vec<String>,
    },
    NotOpaque {
        path: PathBuf,
        scale: f32,
        pixels: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum BadIdReason {
    /// Not of the form `kind.name`.
    Format,
    UnknownKind(String),
    /// `name` is not a `snake_case` Rust identifier.
    BadName(String),
    /// A `radio.` id that isn't `radio.<group>.<position>`.
    RadioFormat,
    /// A `tab.` id that isn't `tab.<group>.<position>`.
    TabFormat,
}

impl fmt::Display for BuildError {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            BuildError::FactoryPreset { path, problem } => {
                write!(f, "{}: {problem}", path.display())
            }
            BuildError::BitmapFont {
                path,
                line: Some(line),
                problem,
            } => write!(f, "{}:{line}: {problem}", path.display()),
            BuildError::BitmapFont {
                path,
                line: None,
                problem,
            } => write!(f, "{}: {problem}", path.display()),
            BuildError::NoOutDir => write!(
                f,
                "OUT_DIR is not set. Call Faceplate::generate from build.rs, or set an output directory with .out_dir(..)"
            ),
            BuildError::Xml { path, message } => {
                write!(f, "{}: not well-formed XML: {message}", path.display())
            }
            BuildError::Svg { path, message } => {
                write!(f, "{}: could not parse SVG: {message}", path.display())
            }
            BuildError::NoControlsLayer { path } => write!(
                f,
                "{}: no controls layer. Add a group with id=\"controls\" (or an Inkscape layer labelled \"controls\") \
                 containing one shape per control, each with an id like \"knob.gain\"",
                path.display()
            ),
            BuildError::MultipleControlsLayers { path, lines } => write!(
                f,
                "{}: more than one controls layer (lines {}). Merge them into a single group with id=\"controls\"",
                path.display(),
                lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            BuildError::ControlsLayerHidden { path, line } => write!(
                f,
                "{}:{line}: the controls layer is hidden (display:none or visibility:hidden). \
                 Make it visible: silkscreen excludes it from the faceplate render automatically, \
                 but needs its geometry to place the controls",
                path.display()
            ),
            BuildError::MarkerWithoutId {
                path,
                line,
                element,
            } => write!(
                f,
                "{}:{line}: <{element}> in the controls layer has no id. \
                 Give it an id of the form \"kind.name\", e.g. id=\"knob.gain\"",
                path.display()
            ),
            BuildError::BadControlId {
                path,
                line,
                id,
                reason,
            } => {
                write!(f, "{}:{line}: control id \"{id}\" ", path.display())?;
                match reason {
                    BadIdReason::Format => write!(
                        f,
                        "is not of the form \"kind.name\". Rename it, e.g. id=\"knob.{}\"",
                        suggest_name(id)
                    ),
                    BadIdReason::UnknownKind(kind) => write!(
                        f,
                        "has unknown kind \"{kind}\". Supported kinds: {}; radio buttons are \
                         \"radio.<group>.<position>\"",
                        silkscreen_core::ControlKind::ALL
                            .iter()
                            .map(|k| k.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    BadIdReason::RadioFormat => write!(
                        f,
                        "is not of the form \"radio.<group>.<position>\". Radio buttons name their group and \
                         their position from 0, e.g. id=\"radio.tuning.0\", id=\"radio.tuning.1\""
                    ),
                    BadIdReason::TabFormat => write!(
                        f,
                        "is not of the form \"tab.<group>.<position>\". Tabs name their page group and \
                         their page from 0, e.g. id=\"tab.fx.0\", id=\"tab.fx.1\""
                    ),
                    BadIdReason::BadName(name) => write!(
                        f,
                        "has name \"{name}\", which is not a snake_case Rust identifier \
                         (lowercase letters, digits and underscores, starting with a letter, not a keyword). \
                         Try \"{}\"",
                        suggest_name(name)
                    ),
                }
            }
            BuildError::DuplicateName { path, name, lines } => write!(
                f,
                "{}: control name \"{name}\" is used twice (lines {} and {}). Control names must be unique",
                path.display(),
                lines.0,
                lines.1
            ),
            BuildError::UnknownSkin {
                path,
                line,
                id,
                skin,
                known,
            } => write!(
                f,
                "{}:{line}: control \"{id}\" has data-skin=\"{skin}\", which isn't registered. \
                 Add it in build.rs with .skin(\"{skin}\", &[\"frame.svg\", ...]){}",
                path.display(),
                if known.is_empty() {
                    String::new()
                } else {
                    format!(", or use one of: {}", known.join(", "))
                }
            ),
            BuildError::SkinFrames {
                path,
                line,
                id,
                skin,
                expected,
                got,
            } => write!(
                f,
                "{}:{line}: control \"{id}\" uses skin \"{skin}\", which has {got} frame(s); this kind \
                 needs {expected}. Knobs take a base and a rotating top (or a filmstrip of three or more \
                 frames, one per angle); toggles, buttons and radio \
                 buttons take an off frame and an on frame (a three-way toggle: down, middle, up); faders and selectors take their cap",
                path.display()
            ),
            BuildError::SkinUnsupported { path, line, id } => write!(
                f,
                "{}:{line}: control \"{id}\" has a data-skin, but its kind doesn't take skins. \
                 Remove the attribute; it draws as a vector",
                path.display()
            ),
            BuildError::SkinSizes {
                path,
                skin,
                first,
                second,
            } => write!(
                f,
                "{}: skin \"{skin}\" is used by \"{}\" at {}x{} and by \"{}\" at {}x{}. A skin renders \
                 at one size: make the markers the same size, or register a second skin",
                path.display(),
                first.0,
                first.1.w,
                first.1.h,
                second.0,
                second.1.w,
                second.1.h
            ),
            BuildError::BadSkinName { name } => write!(
                f,
                "skin name \"{name}\" is not a snake_case Rust identifier. Rename it in build.rs, e.g. \
                 .skin(\"{}\", ..)",
                suggest_name(name)
            ),
            BuildError::PageGroup {
                path,
                group,
                problem,
            } => write!(f, "{}: page group \"{group}\" {problem}", path.display()),
            BuildError::RadioGap {
                path,
                group,
                missing,
                count,
            } => write!(
                f,
                "{}: radio group \"{group}\" has {count} buttons but no position {missing}. \
                 Number a group's buttons 0, 1, 2, ... with no gaps or repeats, e.g. id=\"radio.{group}.{missing}\"",
                path.display()
            ),
            BuildError::NoGeometry { path, id } => write!(
                f,
                "{}: control marker \"{id}\" has no renderable geometry, so it has no bounding box. \
                 Give it a fill (markers are never drawn on the faceplate)",
                path.display()
            ),
            BuildError::ZeroSize { path, id } => write!(
                f,
                "{}: control marker \"{id}\" has zero width or height. \
                 Draw it as a rect or circle covering the control's area",
                path.display()
            ),
            BuildError::ZeroSizeFaceplate { path } => {
                write!(f, "{}: the SVG has zero width or height", path.display())
            }
            BuildError::MissingFont { path, families } => write!(
                f,
                "{}: no font found for font-family {}. Add the font file to a directory passed to \
                 Faceplate::font_dir, enable Faceplate::system_fonts(true), or convert the text to paths",
                path.display(),
                families.join(", ")
            ),
            BuildError::NotOpaque {
                path,
                scale,
                pixels,
            } => write!(
                f,
                "{}: the faceplate render at {scale}x has {pixels} transparent or translucent pixels. \
                 Faceplates must be fully opaque: add a background rect covering the whole canvas",
                path.display()
            ),
        }
    }
}

impl std::error::Error for BuildError {}

/// A best-effort `snake_case` identifier from arbitrary text.
fn suggest_name(s: &str) -> String {
    let base = s.rsplit('.').next().unwrap_or(s);
    let mut out = String::new();
    for c in base.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase() && !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_owned();
    match out.chars().next() {
        None => "control".to_owned(),
        Some(c) if c.is_ascii_digit() => format!("c_{out}"),
        Some(_) if crate::extract::is_keyword(&out) => format!("{out}_"),
        Some(_) => out,
    }
}
