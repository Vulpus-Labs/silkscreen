//! Colours, sizes and shapes shared by silkscreen's overlays: tooltips,
//! lists, menus, tables, dialogs, the preset browser and fields. Dark by
//! default, with a red for selection; a plugin sets its own [`Theme`]
//! with `EditorState::set_theme`.

use std::cell::Cell;

use silkscreen_core::Rect;
use silkscreen_core::tiny_skia::{
    Color, FillRule, LineCap, Paint, PathBuilder, PixmapMut, Stroke, Transform,
};

/// RGBA.
pub type Rgba = [u8; 4];

/// The overlays' colours. Sizes ([`TEXT_SIZE`], [`RADIUS`]) aren't in
/// it: they set rows' heights and so where input lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Rgba,
    pub border: Rgba,
    pub ink: Rgba,
    /// `ink` for what is shown but not in effect: a dimmed row or choice.
    pub dim_ink: Rgba,
    /// A close button's X when not hovered.
    pub close_ink: Rgba,
    /// A row or button under the pointer.
    pub hover: Rgba,
    /// The selected row, and selected text.
    pub selected: Rgba,
    /// A row flagged as wrong; its text, not a fill.
    pub alert: Rgba,
    pub scroll_track: Rgba,
    pub scroll_thumb: Rgba,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: [24, 24, 27, 255],
            border: [220, 220, 220, 90],
            ink: [237, 237, 237, 255],
            dim_ink: [237, 237, 237, 96],
            close_ink: [180, 180, 186, 255],
            hover: [255, 255, 255, 18],
            selected: [226, 38, 46, 150],
            alert: [255, 107, 96, 255],
            scroll_track: [255, 255, 255, 14],
            scroll_thumb: [255, 255, 255, 70],
        }
    }
}

thread_local! {
    /// The theme overlays draw with. The view sets its editor's for the
    /// length of each render ([`with`]); outside one it is the default.
    static CURRENT: Cell<Theme> = Cell::new(Theme::default());
}

/// The theme to draw with now.
pub(crate) fn theme() -> Theme {
    CURRENT.with(Cell::get)
}

/// The editor's theme, for a plugin's own panels and editor widgets to
/// draw with. Valid while they draw, inside the view's render; outside
/// one (in `input`, `update`) it is the default, and
/// `EditorState::theme` is the one to read.
pub fn current() -> Theme {
    theme()
}

/// `top` composited over opaque `under`.
pub(crate) fn over(top: Rgba, under: Rgba) -> Rgba {
    let a = u16::from(top[3]);
    let mix = |t: u8, u: u8| ((u16::from(t) * a + u16::from(u) * (255 - a) + 127) / 255) as u8;
    [
        mix(top[0], under[0]),
        mix(top[1], under[1]),
        mix(top[2], under[2]),
        under[3],
    ]
}

/// Run `f` drawing with `theme`, then put the previous one back.
pub(crate) fn with<R>(theme: Theme, f: impl FnOnce() -> R) -> R {
    /// Puts the previous theme back however `f` leaves, unwinding too.
    struct Restore(Theme);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(theme)));
    f()
}

/// Runtime text size, in logical units: the overlays' rows are laid out
/// for it, so a plugin's own panel matches them by using it.
pub const TEXT_SIZE: f32 = 11.0;
/// Corner radius of panels and tips.
pub const RADIUS: f32 = 3.0;

/// An [`Rgba`] as tiny-skia's colour, for drawing with a [`Theme`].
pub fn color([r, g, b, a]: Rgba) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// `rect` with corners of `radius`: core's path.
pub(crate) use silkscreen_core::rounded_rect as rounded;

/// A close button's glyph: an X centred in `rect`, over a hover wash
/// when `hover`.
pub(crate) fn draw_close(
    canvas: &mut PixmapMut<'_>,
    rect: Rect,
    hover: bool,
    transform: Transform,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    if hover && let Some(path) = rounded(rect, RADIUS) {
        paint.set_color(color(theme().hover));
        canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
    }
    let c = rect.center();
    let half = (rect.w.min(rect.h) * 0.2).max(2.5);
    let mut pb = PathBuilder::new();
    pb.move_to(c.x - half, c.y - half);
    pb.line_to(c.x + half, c.y + half);
    pb.move_to(c.x + half, c.y - half);
    pb.line_to(c.x - half, c.y + half);
    if let Some(path) = pb.finish() {
        let t = theme();
        paint.set_color(color(if hover { t.ink } else { t.close_ink }));
        let stroke = Stroke {
            width: 1.4,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}
