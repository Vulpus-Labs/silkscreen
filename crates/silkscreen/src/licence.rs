//! The faceplate side of the soft lock ([design/licensing-hook.md]):
//! while a plugin is locked, a small "Unregistered — Unlock…" strip sits
//! in the faceplate's bottom-right corner, drawn by the framework in the
//! theme's lettering. Clicking it opens the [`Unlock`](crate::Unlock)
//! dialog.
//!
//! The editor knows nothing of codes or plugins: the host gives it a
//! [`Licence`] to ask and to unlock with, and a [`Clipboard`] for Paste.
//!
//! [design/licensing-hook.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/licensing-hook.md

use silkscreen_core::tiny_skia::{PixmapMut, Transform};
use silkscreen_core::{Overlay, Rect, Size};
use silkscreen_text::Style;

use crate::dialog::{fill_rounded, outline};
use crate::theme::{TEXT_SIZE, color, theme};
use crate::tooltip::Lettering;

/// A licensed plugin's lock, as the host keeps it.
pub trait Licence {
    /// Whether the plugin is still locked. Asked each tick: keep it cheap.
    fn locked(&self) -> bool;

    /// Check `code` and, if it is good, unlock every instance of the
    /// plugin. The error is a sentence for the dialog to show.
    fn unlock(&self, code: &str) -> Result<(), String>;
}

/// The host's clipboard, for pasting text.
pub trait Clipboard {
    /// The clipboard's text, if it holds any.
    fn text(&self) -> Option<String>;
}

/// What the strip says.
pub const STRIP_TEXT: &str = "Unregistered — Unlock…";

const PAD_X: f32 = 8.0;
const H: f32 = 20.0;
/// From the faceplate's edges, as the badge.
const MARGIN: f32 = 4.0;

/// The strip's logical rect on a faceplate of `size`: bottom-right,
/// mirroring the badge's corner.
pub(crate) fn strip_rect(size: Size, lettering: &Lettering) -> Rect {
    let w = lettering
        .text
        .borrow_mut()
        .width(&lettering.face, STRIP_TEXT, TEXT_SIZE, 1.0)
        .ceil()
        + 2.0 * PAD_X;
    Rect::new(size.w - MARGIN - w, size.h - MARGIN - H, w, H)
}

/// The strip, as an overlay below any panel.
pub(crate) struct Strip {
    pub rect: Rect,
    pub lettering: Lettering,
}

impl Overlay for Strip {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let r = self.rect;
        fill_rounded(canvas, r, theme().background, transform);
        outline(canvas, r, transform);
        let face = &self.lettering.face;
        let (ascent, descent) = face.line_metrics(TEXT_SIZE);
        let style = Style {
            face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        self.lettering.text.borrow_mut().draw_clipped(
            canvas,
            STRIP_TEXT,
            (r.x + PAD_X, r.center().y + f32::midpoint(ascent, descent)),
            &style,
            transform,
            Some([r.x, r.y, r.right(), r.bottom()]),
        );
    }
}
