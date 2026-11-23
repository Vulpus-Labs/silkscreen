//! Tooltips and the drag readout ([design/overlays-and-text.md]): one small label drawn
//! as an overlay below (or above) a control.
//!
//! [design/overlays-and-text.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overlays-and-text.md

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen_core::tiny_skia::{FillRule, Paint, PixmapMut, Stroke, Transform};
use silkscreen_core::{Overlay, Rect, Size};
use silkscreen_text::{Face, Style, Text};

use crate::theme::{RADIUS, TEXT_SIZE, color, rounded, theme};

const PAD_X: f32 = 6.0;
const PAD_Y: f32 = 3.0;
/// Between the control and the tip.
const GAP: f32 = 4.0;
/// Kept clear of the window's edges.
const MARGIN: f32 = 2.0;

/// Shared by every tip a view draws: the face and its glyph cache.
#[derive(Clone)]
pub(crate) struct Lettering {
    pub face: Rc<Face>,
    pub text: Rc<RefCell<Text>>,
}

impl Lettering {
    /// The thread's Jost Medium and glyph cache, shared by every view,
    /// tip and list on it (they're all on the UI thread).
    pub fn shared() -> Self {
        thread_local! {
            static SHARED: Lettering = Lettering {
                face: Rc::new(Face::new(silkscreen_text::JOST_MEDIUM, 0)),
                text: Rc::new(RefCell::new(Text::new())),
            };
        }
        SHARED.with(Clone::clone)
    }
}

/// A label for the control at `control`, placed below it, or above it
/// when there's no room below, and kept inside a window of `window`
/// logical units. `scale` is the view's, for measuring.
pub(crate) struct Tip {
    rect: Rect,
    label: String,
    baseline: (f32, f32),
    lettering: Lettering,
}

impl Tip {
    pub fn new(
        label: String,
        control: Rect,
        window: Size,
        scale: f32,
        lettering: &Lettering,
    ) -> Self {
        let width = lettering
            .text
            .borrow_mut()
            .width(&lettering.face, &label, TEXT_SIZE, scale)
            + 2.0 * PAD_X;
        let (ascent, descent) = lettering.face.line_metrics(TEXT_SIZE);
        let height = ascent - descent + 2.0 * PAD_Y;
        let below = control.bottom() + GAP;
        let y = if below + height <= window.h - MARGIN {
            below
        } else {
            (control.y - GAP - height).max(MARGIN)
        };
        let x = (control.center().x - width / 2.0)
            .min(window.w - MARGIN - width)
            .max(MARGIN);
        Self {
            rect: Rect::new(x, y, width, height),
            baseline: (x + PAD_X, y + PAD_Y + ascent),
            label,
            lettering: lettering.clone(),
        }
    }
}

impl Overlay for Tip {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let r = self.rect;
        // Inset half a line so the border stays inside the rect.
        let inset = 0.5 / transform.sx;
        let radius = RADIUS;
        let inner = Rect::new(
            r.x + inset,
            r.y + inset,
            r.w - 2.0 * inset,
            r.h - 2.0 * inset,
        );
        if let Some(path) = rounded(inner, radius) {
            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };
            paint.set_color(color(theme().background));
            canvas.fill_path(&path, &paint, FillRule::Winding, transform, None);
            paint.set_color(color(theme().border));
            let stroke = Stroke {
                width: 1.0 / transform.sx,
                ..Stroke::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, transform, None);
        }
        let style = Style {
            face: &self.lettering.face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        self.lettering.text.borrow_mut().draw(
            canvas,
            &self.label,
            self.baseline,
            &style,
            transform,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sits_below_the_control_and_stays_in_the_window() {
        let l = Lettering::shared();
        let window = Size::new(200.0, 100.0);
        let tip = Tip::new(
            "Cutoff  0.50".into(),
            Rect::new(80.0, 10.0, 40.0, 40.0),
            window,
            1.0,
            &l,
        );
        let r = tip.rect();
        assert!((r.y - 54.0).abs() < 0.01, "below, with the gap: {r:?}");
        assert!((r.center().x - 100.0).abs() < 0.5, "centred: {r:?}");

        // No room below: above. Near the right edge: pulled in.
        let tip = Tip::new(
            "Cutoff  0.50".into(),
            Rect::new(180.0, 70.0, 20.0, 25.0),
            window,
            1.0,
            &l,
        );
        let r = tip.rect();
        assert!(r.bottom() <= 70.0 - GAP + 0.01, "above: {r:?}");
        assert!(r.right() <= 200.0 - MARGIN + 0.01, "inside: {r:?}");
    }
}
