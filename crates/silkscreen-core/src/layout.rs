//! Faceplate layout: the controls extracted from the SVG ([design/faceplates.md]).
//!
//! These types are instantiated as `static`s by code that `silkscreen-build`
//! generates, so every constructor is `const`.
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

use crate::geom::{Point, Rect, Size};
use crate::raster::{EmbeddedPngs, Skin};

/// Identifies a control: its index in [`Layout::controls`], in SVG document
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ControlId(pub u16);

impl ControlId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The kind of a control, from the `kind` half of its SVG id
/// ([design/model.md]).
///
/// [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md
/// What the mouse cursor shows: what a press here would do (design/overlays-and-text.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseCursor {
    /// The arrow: nothing to do here.
    #[default]
    Default,
    /// A click acts, or a handle can be taken.
    Hand,
    /// A handle being dragged.
    Grabbing,
    /// A drag up and down: knobs and faders.
    Vertical,
    /// A drag left and right: a table's value cells.
    Horizontal,
    /// Typed text.
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlKind {
    Knob,
    /// A vertical slider.
    Fader,
    /// A detented vertical slider over a stepped param.
    Selector,
    /// A two-position bat switch.
    Toggle,
    /// A latching LED button.
    Button,
    /// A field showing a stepped param's current choice that opens a list
    /// of the choices.
    DropDown,
    /// A field showing the current preset's name that opens the preset
    /// browser, with ‹ › at its ends stepping through presets
    /// (design/presets.md).
    Preset,
    /// A momentary push area, from `action.<name>`: the faceplate's art
    /// labels it, the widget washes it on hover and press, and a binding
    /// says what a click does (e.g. the preset bar's Save, design/presets.md).
    Action,
    /// A triggered waveform display of the engine's output ([design/engine-bridge.md]).
    /// Display only.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    Scope,
    /// A peak meter of the engine's output ([design/engine-bridge.md]). Display only.
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    Meter,
    /// A region the plugin paints itself, from `canvas.<name>`
    /// ([design/faceplates.md]): an envelope, a graph, a custom display. Its
    /// widget comes from the plugin (`EditorState::set_widget`); its
    /// binding handles input like any control's.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    Canvas,
    /// One button of a radio group, from `radio.<group>.<position>`. The
    /// group's buttons share one stepped param; this one selects
    /// `position`.
    Radio {
        group: &'static str,
        position: u16,
    },
    /// One tab of a page group, from `tab.<group>.<position>`
    /// ([design/faceplates.md]): a click shows page `position` of `group`.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    Tab {
        group: &'static str,
        position: u16,
    },
}

impl ControlKind {
    /// Every kind spelled `kind.name`, for build-time validation messages.
    /// Radio buttons are spelled `radio.<group>.<position>` instead.
    pub const ALL: &'static [ControlKind] = &[
        ControlKind::Knob,
        ControlKind::Fader,
        ControlKind::Selector,
        ControlKind::Toggle,
        ControlKind::Button,
        ControlKind::DropDown,
        ControlKind::Preset,
        ControlKind::Action,
        ControlKind::Scope,
        ControlKind::Meter,
        ControlKind::Canvas,
    ];

    /// The `kind` spelling used in SVG ids.
    pub const fn as_str(self) -> &'static str {
        match self {
            ControlKind::Knob => "knob",
            ControlKind::Fader => "fader",
            ControlKind::Selector => "selector",
            ControlKind::Toggle => "toggle",
            ControlKind::Button => "button",
            ControlKind::DropDown => "dropdown",
            ControlKind::Preset => "preset",
            ControlKind::Action => "action",
            ControlKind::Scope => "scope",
            ControlKind::Meter => "meter",
            ControlKind::Canvas => "canvas",
            ControlKind::Radio { .. } => "radio",
            ControlKind::Tab { .. } => "tab",
        }
    }

    /// Draws live data and takes no input: the pointer passes through it.
    pub const fn is_display(self) -> bool {
        matches!(self, ControlKind::Scope | ControlKind::Meter)
    }

    /// A `kind.name` kind; not `radio` or `tab`, which need a group and
    /// position.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlSpec {
    pub id: ControlId,
    pub name: &'static str,
    pub kind: ControlKind,
    /// Absolute bounding box of the control's marker in the SVG.
    pub rect: Rect,
    /// Sprite frames to draw with, from the marker's `data-skin`; `None`
    /// draws the kind's vector widget.
    pub skin: Option<&'static Skin>,
    /// The page the control is on, if any: shown and hit only while its
    /// page is ([design/faceplates.md]).
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    pub page: Option<PageRef>,
}

/// A page of a [`Layout`]'s page group: `(group index, page)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRef {
    pub group: u16,
    pub page: u16,
}

/// A region of the faceplate showing one of several pages ([design/faceplates.md]).
/// Each page is the whole faceplate with that page's art, of which only
/// `region` is used.
///
/// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
#[derive(Debug)]
pub struct PageGroup {
    pub name: &'static str,
    /// Logical bounds of every page's art.
    pub region: Rect,
    pub pages: &'static [EmbeddedPngs],
}

impl PageGroup {
    pub const fn new(name: &'static str, region: Rect, pages: &'static [EmbeddedPngs]) -> Self {
        Self {
            name,
            region,
            pages,
        }
    }
}

impl ControlSpec {
    pub const fn new(id: ControlId, name: &'static str, kind: ControlKind, rect: Rect) -> Self {
        Self {
            id,
            name,
            kind,
            rect,
            skin: None,
            page: None,
        }
    }

    pub const fn with_skin(mut self, skin: &'static Skin) -> Self {
        self.skin = Some(skin);
        self
    }

    pub const fn on_page(mut self, group: u16, page: u16) -> Self {
        self.page = Some(PageRef { group, page });
        self
    }

    /// Whether the control shows when each group `g` shows page
    /// `shown[g]` (page 0 for groups past the end).
    pub fn is_shown(&self, shown: &[u16]) -> bool {
        self.page
            .is_none_or(|p| shown.get(usize::from(p.group)).copied().unwrap_or(0) == p.page)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// Logical size of the faceplate.
    pub size: Size,
    /// Controls in SVG document order; `controls[i].id == ControlId(i)`.
    pub controls: &'static [ControlSpec],
    /// Page groups, by [`PageRef::group`].
    pub pages: &'static [PageGroup],
}

impl ControlKind {
    /// The cursor over a control of this kind, unless its binding says
    /// otherwise ([`Binding::cursor`](../../silkscreen_model/binding/trait.Binding.html)).
    pub fn cursor(self) -> MouseCursor {
        match self {
            Self::Knob | Self::Fader | Self::Selector => MouseCursor::Vertical,
            Self::Toggle
            | Self::Button
            | Self::DropDown
            | Self::Preset
            | Self::Action
            | Self::Radio { .. }
            | Self::Tab { .. } => MouseCursor::Hand,
            Self::Scope | Self::Meter | Self::Canvas => MouseCursor::Default,
        }
    }
}

impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.controls == other.controls
            && std::ptr::eq(self.pages, other.pages)
    }
}

impl Layout {
    pub const fn new(size: Size, controls: &'static [ControlSpec]) -> Self {
        Self {
            size,
            controls,
            pages: &[],
        }
    }

    pub const fn with_pages(mut self, pages: &'static [PageGroup]) -> Self {
        self.pages = pages;
        self
    }

    /// The tabs of page group `group`, in position order.
    pub fn tabs(&self, group: u16) -> Vec<&ControlSpec> {
        let Some(name) = self.pages.get(usize::from(group)).map(|g| g.name) else {
            return Vec::new();
        };
        let mut tabs: Vec<&ControlSpec> = self
            .controls
            .iter()
            .filter(|c| matches!(c.kind, ControlKind::Tab { group: g, .. } if g == name))
            .collect();
        tabs.sort_by_key(|c| match c.kind {
            ControlKind::Tab { position, .. } => position,
            _ => 0,
        });
        tabs
    }

    /// The index of page group `name`.
    pub fn page_group(&self, name: &str) -> Option<u16> {
        self.pages
            .iter()
            .position(|g| g.name == name)
            .and_then(|i| u16::try_from(i).ok())
    }

    pub fn control(&self, id: ControlId) -> &ControlSpec {
        &self.controls[id.index()]
    }

    /// The buttons of radio group `group`, in position order.
    pub fn radio_group(&self, group: &str) -> Vec<&ControlSpec> {
        let mut buttons: Vec<&ControlSpec> = self
            .controls
            .iter()
            .filter(|c| matches!(c.kind, ControlKind::Radio { group: g, .. } if g == group))
            .collect();
        buttons.sort_by_key(|c| match c.kind {
            ControlKind::Radio { position, .. } => position,
            _ => 0,
        });
        buttons
    }

    #[cfg(test)]
    pub fn by_name(&self, name: &str) -> Option<&ControlSpec> {
        self.controls.iter().find(|c| c.name == name)
    }

    /// The control under `p`, with every page group on its first page. See
    /// [`hit_test_shown`](Self::hit_test_shown).
    #[cfg(test)]
    pub fn hit_test(&self, p: Point) -> Option<ControlId> {
        self.hit_test_shown(p, &[])
    }

    /// The control under `p` while each group `g` shows page `shown[g]`.
    /// Later controls are drawn on top, so they win. Display-only controls
    /// and controls on hidden pages aren't hit.
    pub fn hit_test_shown(&self, p: Point, shown: &[u16]) -> Option<ControlId> {
        self.controls
            .iter()
            .rev()
            .find(|c| !c.kind.is_display() && c.is_shown(shown) && c.rect.contains(p))
            .map(|c| c.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CONTROLS: [ControlSpec; 3] = [
        ControlSpec::new(
            ControlId(0),
            "a",
            ControlKind::Knob,
            Rect::new(0.0, 0.0, 20.0, 20.0),
        ),
        ControlSpec::new(
            ControlId(1),
            "b",
            ControlKind::Knob,
            Rect::new(10.0, 10.0, 20.0, 20.0),
        ),
        ControlSpec::new(
            ControlId(2),
            "c",
            ControlKind::Knob,
            Rect::new(50.0, 50.0, 10.0, 10.0),
        ),
    ];
    static LAYOUT: Layout = Layout::new(Size::new(100.0, 100.0), &CONTROLS);

    #[test]
    fn hit_test_topmost_wins() {
        assert_eq!(LAYOUT.hit_test(Point::new(5.0, 5.0)), Some(ControlId(0)));
        // Overlap of a and b: b is later, so on top.
        assert_eq!(LAYOUT.hit_test(Point::new(15.0, 15.0)), Some(ControlId(1)));
        assert_eq!(LAYOUT.hit_test(Point::new(25.0, 25.0)), Some(ControlId(1)));
        assert_eq!(LAYOUT.hit_test(Point::new(40.0, 40.0)), None);
    }

    #[test]
    fn hidden_pages_are_not_hit() {
        static PAGED: [ControlSpec; 2] = [
            ControlSpec::new(
                ControlId(0),
                "a",
                ControlKind::Knob,
                Rect::new(0.0, 0.0, 20.0, 20.0),
            )
            .on_page(0, 0),
            ControlSpec::new(
                ControlId(1),
                "b",
                ControlKind::Knob,
                Rect::new(0.0, 0.0, 20.0, 20.0),
            )
            .on_page(0, 1),
        ];
        static PAGED_LAYOUT: Layout = Layout::new(Size::new(100.0, 100.0), &PAGED);
        let p = Point::new(5.0, 5.0);
        assert_eq!(PAGED_LAYOUT.hit_test(p), Some(ControlId(0)));
        assert_eq!(PAGED_LAYOUT.hit_test_shown(p, &[1]), Some(ControlId(1)));
        assert_eq!(PAGED_LAYOUT.hit_test_shown(p, &[2]), None);
    }

    #[test]
    fn lookup_by_name_and_id() {
        assert_eq!(LAYOUT.by_name("c").map(|c| c.id), Some(ControlId(2)));
        assert_eq!(LAYOUT.control(ControlId(1)).name, "b");
        assert!(LAYOUT.by_name("zzz").is_none());
    }

    #[test]
    fn kind_round_trips() {
        for k in ControlKind::ALL {
            assert_eq!(ControlKind::parse(k.as_str()), Some(*k));
        }
        assert_eq!(ControlKind::parse("slider"), None);
    }
}
