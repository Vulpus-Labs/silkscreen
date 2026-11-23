//! Finding the controls layer and its markers.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use silkscreen_core::{ControlKind, Rect};

use crate::Control;
use crate::error::{BadIdReason, BuildError};

const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";

pub(crate) struct Marker {
    pub id: String,
    pub name: String,
    pub kind: ControlKind,
    /// The marker's `data-skin`, if any.
    pub skin: Option<String>,
    /// The page it's on: `(group, page)`, from an enclosing
    /// `<g data-page="group.page">` in the controls layer.
    pub page: Option<(String, u16)>,
    pub line: u32,
}

/// A page's art: a `<g id="page.<group>.<page>">` outside the controls
/// layer ([design/faceplates.md]).
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
pub(crate) struct PageArt {
    pub group: String,
    pub page: u16,
    pub id: String,
    /// Byte range of the group element in the source text.
    pub range: Range<usize>,
}

pub(crate) struct ControlsLayer {
    /// Byte range of the layer element in the source text.
    pub range: Range<usize>,
    pub markers: Vec<Marker>,
    /// Every page's art, in document order.
    pub pages: Vec<PageArt>,
}

pub(crate) fn controls_layer(path: &Path, text: &str) -> Result<ControlsLayer, BuildError> {
    let opt = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, opt).map_err(|e| BuildError::Xml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let line = |n: roxmltree::Node| doc.text_pos_at(n.range().start).row;

    let layers: Vec<roxmltree::Node> = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && (n.attribute("id") == Some("controls")
                    || n.attribute((INKSCAPE_NS, "label")) == Some("controls"))
        })
        .collect();
    let layer = match layers.as_slice() {
        [] => {
            return Err(BuildError::NoControlsLayer {
                path: path.to_path_buf(),
            });
        }
        [one] => *one,
        many => {
            return Err(BuildError::MultipleControlsLayers {
                path: path.to_path_buf(),
                lines: many.iter().map(|n| line(*n)).collect(),
            });
        }
    };
    if layer.ancestors().any(is_hidden) {
        return Err(BuildError::ControlsLayerHidden {
            path: path.to_path_buf(),
            line: line(layer),
        });
    }

    let mut markers = Vec::new();
    let mut seen: HashMap<String, u32> = HashMap::new();
    let children = page_children(path, &doc, layer)?;
    for (child, page) in children {
        let Some(id) = child.attribute("id") else {
            return Err(BuildError::MarkerWithoutId {
                path: path.to_path_buf(),
                line: line(child),
                element: child.tag_name().name().to_owned(),
            });
        };
        let bad = |reason| BuildError::BadControlId {
            path: path.to_path_buf(),
            line: line(child),
            id: id.to_owned(),
            reason,
        };
        let (kind, name) = if let Some(rest) = id.strip_prefix("radio.") {
            radio(rest).map_err(bad)?
        } else if let Some(rest) = id.strip_prefix("tab.") {
            tab(rest).map_err(bad)?
        } else {
            let Some((kind, name)) = id.split_once('.') else {
                return Err(bad(BadIdReason::Format));
            };
            if name.is_empty() || name.contains('.') || kind.is_empty() {
                return Err(bad(BadIdReason::Format));
            }
            let kind = ControlKind::parse(kind)
                .ok_or_else(|| bad(BadIdReason::UnknownKind(kind.to_owned())))?;
            if !is_snake_ident(name) {
                return Err(bad(BadIdReason::BadName(name.to_owned())));
            }
            (kind, name.to_owned())
        };
        let name = name.as_str();
        if let Some(&first) = seen.get(name) {
            return Err(BuildError::DuplicateName {
                path: path.to_path_buf(),
                name: name.to_owned(),
                lines: (first, line(child)),
            });
        }
        seen.insert(name.to_owned(), line(child));
        markers.push(Marker {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
            skin: child.attribute("data-skin").map(str::to_owned),
            page,
            line: line(child),
        });
    }

    let pages = page_arts(path, &doc, layer)?;

    check_radio_groups(path, &markers)?;
    check_page_groups(path, &markers, &pages)?;
    Ok(ControlsLayer {
        range: layer.range(),
        markers,
        pages,
    })
}

/// A marker and the page it's on, `(group, page)`.
type Paged<'a, 'input> = (roxmltree::Node<'a, 'input>, Option<(String, u16)>);

/// A controls layer's markers, each with its page: the layer's children
/// are markers, or `<g data-page>` groups of them.
fn page_children<'a, 'input>(
    path: &Path,
    doc: &roxmltree::Document<'input>,
    layer: roxmltree::Node<'a, 'input>,
) -> Result<Vec<Paged<'a, 'input>>, BuildError> {
    let mut children = Vec::new();
    for child in layer.children().filter(roxmltree::Node::is_element) {
        if let Some(page) = child.attribute("data-page") {
            let parsed = page
                .split_once('.')
                .and_then(|(g, n)| Some((g, n.parse::<u16>().ok()?)))
                .filter(|(g, _)| is_snake_ident(g));
            let Some((group, n)) = parsed else {
                return Err(BuildError::PageGroup {
                    path: path.to_path_buf(),
                    group: page.to_owned(),
                    problem: format!(
                        "(line {}): data-page=\"{page}\" is not of the form \"<group>.<page>\". \
                         Write e.g. data-page=\"fx.0\"",
                        doc.text_pos_at(child.range().start).row
                    ),
                });
            };
            for marker in child.children().filter(roxmltree::Node::is_element) {
                children.push((marker, Some((group.to_owned(), n))));
            }
        } else {
            children.push((child, None));
        }
    }
    Ok(children)
}

/// Every `<g id="page.<group>.<page>">` outside the controls layer.
fn page_arts(
    path: &Path,
    doc: &roxmltree::Document,
    layer: roxmltree::Node,
) -> Result<Vec<PageArt>, BuildError> {
    let mut pages = Vec::new();
    for n in doc.descendants().filter(roxmltree::Node::is_element) {
        let Some(rest) = n.attribute("id").and_then(|id| id.strip_prefix("page.")) else {
            continue;
        };
        let parsed = rest
            .split_once('.')
            .and_then(|(g, p)| Some((g, p.parse::<u16>().ok()?)))
            .filter(|(g, _)| is_snake_ident(g));
        let Some((group, page)) = parsed else {
            return Err(BuildError::PageGroup {
                path: path.to_path_buf(),
                group: rest.to_owned(),
                problem: format!(
                    "(line {}): id=\"page.{rest}\" is not of the form \"page.<group>.<page>\". \
                     Write e.g. id=\"page.fx.0\"",
                    doc.text_pos_at(n.range().start).row
                ),
            });
        };
        if n.ancestors().any(|a| a == layer) {
            return Err(BuildError::PageGroup {
                path: path.to_path_buf(),
                group: group.to_owned(),
                problem: format!(
                    "(line {}): page art id=\"page.{rest}\" is inside the controls layer. \
                     Put a page's art outside it, and its controls in <g data-page=\"{rest}\">",
                    doc.text_pos_at(n.range().start).row
                ),
            });
        }
        pages.push(PageArt {
            group: group.to_owned(),
            page,
            id: format!("page.{rest}"),
            range: n.range(),
        });
    }

    Ok(pages)
}

/// `<group>.<position>` of a `tab.<group>.<position>` id: the kind, and
/// the control name `<group>_<position>`.
fn tab(rest: &str) -> Result<(ControlKind, String), BadIdReason> {
    match radio(rest) {
        Ok((ControlKind::Radio { group, position }, name)) => {
            Ok((ControlKind::Tab { group, position }, name))
        }
        _ => Err(BadIdReason::TabFormat),
    }
}

/// One page group's tabs and pages are both exactly `0..n`, and its
/// controls are on one of its pages.
fn check_group(
    path: &Path,
    group: &str,
    markers: &[Marker],
    pages: &[PageArt],
) -> Result<(), BuildError> {
    let err = |group: &str, problem: String| BuildError::PageGroup {
        path: path.to_path_buf(),
        group: group.to_owned(),
        problem,
    };
    let mut tabs: Vec<u16> = markers
        .iter()
        .filter_map(|m| match m.kind {
            ControlKind::Tab { group: g, position } if g == group => Some(position),
            _ => None,
        })
        .collect();
    let mut arts: Vec<u16> = pages
        .iter()
        .filter(|p| p.group == group)
        .map(|p| p.page)
        .collect();
    tabs.sort_unstable();
    arts.sort_unstable();
    let gap = |v: &[u16]| {
        (0u16..)
            .zip(v)
            .find(|(want, got)| want != *got)
            .map(|(w, _)| w)
    };
    if tabs.is_empty() {
        return Err(err(
            group,
            "has pages but no tabs. Add a control id=\"tab.<group>.<page>\" for each page".into(),
        ));
    }
    if let Some(missing) = gap(&tabs) {
        return Err(err(
            group,
            format!(
                "has {} tabs but no tab {missing}. Number a group's tabs 0, 1, 2, ... with no \
                 gaps or repeats, e.g. id=\"tab.{group}.{missing}\"",
                tabs.len()
            ),
        ));
    }
    if let Some(missing) =
        gap(&arts).or_else(|| (arts.len() < tabs.len()).then_some(arts.len() as u16))
    {
        return Err(err(
            group,
            format!(
                "has {} tabs but no page {missing}. Give each tab its page's art in \
                 <g id=\"page.{group}.{missing}\">, outside the controls layer",
                tabs.len()
            ),
        ));
    }
    if arts.len() > tabs.len() {
        return Err(err(
            group,
            format!(
                "has {} pages but {} tabs. Add id=\"tab.{group}.{}\", or remove the page",
                arts.len(),
                tabs.len(),
                tabs.len()
            ),
        ));
    }
    if let Some(m) = markers.iter().find(|m| {
        m.page
            .as_ref()
            .is_some_and(|(g, n)| g == group && usize::from(*n) >= tabs.len())
    }) {
        let (_, n) = m.page.as_ref().expect("found by its page");
        return Err(err(
            group,
            format!(
                "has {} pages, but control \"{}\" (line {}) is on page {n}. Put it in an \
                 existing page's <g data-page>",
                tabs.len(),
                m.id,
                m.line
            ),
        ));
    }
    Ok(())
}

/// Each page group's tabs and pages are both exactly `0..n`, and its
/// controls are on one of its pages.
fn check_page_groups(path: &Path, markers: &[Marker], pages: &[PageArt]) -> Result<(), BuildError> {
    let mut groups: Vec<&str> = Vec::new();
    let tab_groups = markers.iter().filter_map(|m| match m.kind {
        ControlKind::Tab { group, .. } => Some(group),
        _ => None,
    });
    for g in tab_groups
        .chain(pages.iter().map(|p| p.group.as_str()))
        .chain(
            markers
                .iter()
                .filter_map(|m| m.page.as_ref().map(|(g, _)| g.as_str())),
        )
    {
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    for group in groups {
        check_group(path, group, markers, pages)?;
    }
    Ok(())
}

/// `<group>.<position>` of a `radio.<group>.<position>` id: the kind, and
/// the control name `<group>_<position>`.
fn radio(rest: &str) -> Result<(ControlKind, String), BadIdReason> {
    let Some((group, position)) = rest.split_once('.') else {
        return Err(BadIdReason::RadioFormat);
    };
    if !is_snake_ident(group) {
        return Err(BadIdReason::BadName(group.to_owned()));
    }
    let position: u16 = position.parse().map_err(|_| BadIdReason::RadioFormat)?;
    // Generated code needs `&'static str` groups. A build script is a
    // short-lived process, so leaking each group's name once is fine.
    let group: &'static str = Box::leak(group.to_owned().into_boxed_str());
    Ok((
        ControlKind::Radio { group, position },
        format!("{group}_{position}"),
    ))
}

/// Each radio group's positions must be exactly `0..n`.
fn check_radio_groups(path: &Path, markers: &[Marker]) -> Result<(), BuildError> {
    let mut groups: Vec<(&str, Vec<u16>)> = Vec::new();
    for m in markers {
        if let ControlKind::Radio { group, position } = m.kind {
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, positions)) => positions.push(position),
                None => groups.push((group, vec![position])),
            }
        }
    }
    for (group, mut positions) in groups {
        positions.sort_unstable();
        if let Some(missing) = (0u16..).zip(&positions).find(|(want, got)| want != *got) {
            return Err(BuildError::RadioGap {
                path: path.to_path_buf(),
                group: group.to_owned(),
                missing: missing.0,
                count: positions.len(),
            });
        }
    }
    Ok(())
}

pub(crate) fn control_rects(
    path: &Path,
    tree: &usvg::Tree,
    markers: &[Marker],
) -> Result<Vec<Control>, BuildError> {
    markers
        .iter()
        .map(|m| {
            let node = tree
                .node_by_id(&m.id)
                .ok_or_else(|| BuildError::NoGeometry {
                    path: path.to_path_buf(),
                    id: m.id.clone(),
                })?;
            let b = node.abs_bounding_box();
            if b.width() <= 0.0 || b.height() <= 0.0 {
                return Err(BuildError::ZeroSize {
                    path: path.to_path_buf(),
                    id: m.id.clone(),
                });
            }
            Ok(Control {
                name: m.name.clone(),
                kind: m.kind,
                rect: Rect::new(b.x(), b.y(), b.width(), b.height()),
                skin: m.skin.clone(),
                page: m.page.clone(),
                id: m.id.clone(),
                line: m.line,
            })
        })
        .collect()
}

fn is_hidden(n: roxmltree::Node) -> bool {
    if n.attribute("display") == Some("none") || n.attribute("visibility") == Some("hidden") {
        return true;
    }
    n.attribute("style").is_some_and(|style| {
        style.split(';').any(|decl| {
            let mut kv = decl.splitn(2, ':').map(str::trim);
            matches!(
                (kv.next(), kv.next()),
                (Some("display"), Some("none")) | (Some("visibility"), Some("hidden"))
            )
        })
    })
}

fn is_snake_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !is_keyword(s)
}

pub(crate) fn is_snake_ident_pub(s: &str) -> bool {
    is_snake_ident(s)
}

pub(crate) fn is_keyword(s: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
    ];
    KEYWORDS.contains(&s)
}
