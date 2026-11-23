//! A scrolling list panel: rows of text taller than the panel,
//! scrolled by wheel, scrollbar or keys, with hover and selection. The
//! base of drop-downs, the preset browser and the mod matrix's pickers.

use silkscreen_core::tiny_skia::{FillRule, Paint, PixmapMut, Rect as SkRect, Stroke, Transform};
use silkscreen_core::{Key, MouseCursor, Point, RawKind, Rect};
use silkscreen_text::Style;

use crate::EditorState;
use crate::panel::{Panel, Reply};
use crate::theme::{RADIUS, Rgba, TEXT_SIZE, color, rounded, theme};
use crate::tooltip::Lettering;

/// Row height, in logical units.
pub const ROW_H: f32 = 18.0;
const PAD_X: f32 = 8.0;
/// Inside the border, above the first row and below the last.
const PAD_Y: f32 = 2.0;
const BAR_W: f32 = 5.0;
const BAR_MARGIN: f32 = 2.0;
const THUMB_MIN: f32 = 16.0;

/// The scrollbar of a view scrolled `scroll` down content `content_h`
/// tall: its track and thumb, when the content overflows. The list's and
/// the table's. The thumb's edges are on whole pixels at `scale`: a
/// fractional edge anti-aliases differently in a partial repaint.
pub(crate) fn scrollbar(
    view: Rect,
    content_h: f32,
    scroll: f32,
    scale: f32,
) -> Option<(Rect, Rect)> {
    let max = content_h - view.h;
    if max <= 0.0 {
        return None;
    }
    let track = Rect::new(
        view.right() - BAR_MARGIN - BAR_W,
        view.y + BAR_MARGIN,
        BAR_W,
        view.h - 2.0 * BAR_MARGIN,
    );
    let px = |v: f32| (v * scale).round() / scale;
    let thumb_h = (track.h * view.h / content_h).max(THUMB_MIN).min(track.h);
    let top = px(track.y + (track.h - thumb_h) * scroll / max);
    Some((
        track,
        Rect::new(track.x, top, BAR_W, px(top + thumb_h) - top),
    ))
}

type Choose = Box<dyn FnMut(usize, &mut EditorState) -> Reply>;

/// Draws a choice's picture (a curve, a waveform) in `rect`, in `ink`:
/// `draw(canvas, rect, choice, ink, transform)`. A [`List`] and a table's
/// [`Cell::GlyphChoice`](crate::Cell::GlyphChoice) show it beside the
/// choice's label.
pub type Glyph = std::rc::Rc<dyn Fn(&mut PixmapMut<'_>, Rect, usize, Rgba, Transform)>;

/// A glyph's box beside a label in a row `row_h` tall: as wide as it is
/// tall, and a half more.
pub(crate) fn glyph_size(row_h: f32) -> (f32, f32) {
    let h = row_h - 6.0;
    (h * 1.5, h)
}

/// A list of labelled rows in a panel. Choosing a row (click, or Enter on
/// the selection) calls `on_choose`, whose reply the panel returns, so a
/// drop-down can close on choice and a browser can stay open.
pub struct List {
    rect: Rect,
    items: Vec<String>,
    scroll: f32,
    selected: Option<usize>,
    /// Rows drawn dimmed, still chosen as any other; empty for none.
    dimmed: Vec<bool>,
    hover: Option<usize>,
    /// The row pressed, chosen if released over the same row.
    pressed: Option<usize>,
    /// A thumb drag: the pointer's y and the scroll when it began.
    dragging: Option<(f32, f32)>,
    lettering: Lettering,
    on_choose: Choose,
    glyph: Option<Glyph>,
}

impl std::fmt::Debug for List {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("List")
            .field("rect", &self.rect)
            .field("items", &self.items.len())
            .field("scroll", &self.scroll)
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

impl List {
    pub fn new(
        rect: Rect,
        items: Vec<String>,
        on_choose: impl FnMut(usize, &mut EditorState) -> Reply + 'static,
    ) -> Self {
        Self {
            rect,
            items,
            scroll: 0.0,
            selected: None,
            dimmed: Vec::new(),
            hover: None,
            pressed: None,
            dragging: None,
            lettering: Lettering::shared(),
            on_choose: Box::new(on_choose),
            glyph: None,
        }
    }

    /// Draw each row's glyph before its label.
    #[must_use]
    pub fn with_glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Draw the rows `dimmed` marks in dim ink; they can still be chosen.
    #[must_use]
    pub fn with_dimmed(mut self, dimmed: Vec<bool>) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// Start with row `index` selected and scrolled into view (centred if
    /// it can be).
    #[must_use]
    pub fn with_selected(mut self, index: usize) -> Self {
        if index < self.items.len() {
            self.selected = Some(index);
            let view = self.view();
            self.set_scroll(index as f32 * ROW_H - (view.h - ROW_H) / 2.0);
        }
        self
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// How far the rows are scrolled, in logical units.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The rows' viewport: inside the border and padding.
    fn view(&self) -> Rect {
        let r = self.rect;
        Rect::new(
            r.x + 1.0,
            r.y + 1.0 + PAD_Y,
            r.w - 2.0,
            r.h - 2.0 - 2.0 * PAD_Y,
        )
    }

    fn content_h(&self) -> f32 {
        self.items.len() as f32 * ROW_H
    }

    fn max_scroll(&self) -> f32 {
        (self.content_h() - self.view().h).max(0.0)
    }

    fn set_scroll(&mut self, scroll: f32) -> bool {
        let s = scroll.clamp(0.0, self.max_scroll());
        let changed = s != self.scroll;
        self.scroll = s;
        changed
    }

    /// The scrollbar's track and thumb, when the rows overflow. The
    /// thumb is on whole units, so input and drawing agree at any scale.
    fn bar(&self) -> Option<(Rect, Rect)> {
        scrollbar(self.view(), self.content_h(), self.scroll, 1.0)
    }

    /// The row under `pos`, if any.
    /// Row `i`'s band, cut to the viewport; `None` if scrolled out.
    fn row_rect(&self, i: usize) -> Option<Rect> {
        let view = self.view();
        let top = view.y + i as f32 * ROW_H - self.scroll;
        Rect::new(view.x, top, view.w, ROW_H).intersect(&view)
    }

    fn row_at(&self, pos: Point) -> Option<usize> {
        let view = self.view();
        if !view.contains(pos) || self.bar().is_some_and(|(track, _)| pos.x >= track.x - 1.0) {
            return None;
        }
        let i = ((pos.y - view.y + self.scroll) / ROW_H).floor();
        (i >= 0.0 && (i as usize) < self.items.len()).then_some(i as usize)
    }

    /// Rows that fit in the viewport.
    fn page(&self) -> usize {
        ((self.view().h / ROW_H).floor() as usize).max(1)
    }

    /// Scroll so row `i` is wholly visible.
    fn reveal(&mut self, i: usize) {
        let view = self.view();
        let top = i as f32 * ROW_H;
        if top < self.scroll {
            self.set_scroll(top);
        } else if top + ROW_H > self.scroll + view.h {
            self.set_scroll(top + ROW_H - view.h);
        }
    }

    fn select(&mut self, i: usize) {
        self.selected = Some(i);
        self.reveal(i);
    }

    fn key(&mut self, key: Key, state: &mut EditorState) -> Reply {
        let n = self.items.len();
        if n == 0 {
            return Reply::Ignored;
        }
        let at = self.selected;
        let last = n - 1;
        let to = match key {
            Key::ArrowDown => at.map_or(0, |i| (i + 1).min(last)),
            Key::ArrowUp => at.map_or(0, |i| i.saturating_sub(1)),
            Key::PageDown => at.map_or(0, |i| (i + self.page()).min(last)),
            Key::PageUp => at.map_or(0, |i| i.saturating_sub(self.page())),
            Key::Home => 0,
            Key::End => last,
            Key::Enter => {
                return match at {
                    Some(i) => (self.on_choose)(i, state),
                    None => Reply::Ignored,
                };
            }
            _ => return Reply::Ignored,
        };
        self.select(to);
        Reply::Handled
    }

    fn fill(canvas: &mut PixmapMut<'_>, r: Rect, rgba: Rgba, transform: Transform) {
        if let Some(rect) = SkRect::from_xywh(r.x, r.y, r.w, r.h) {
            let mut paint = Paint::default();
            paint.set_color(color(rgba));
            canvas.fill_rect(rect, &paint, transform, None);
        }
    }
}

impl Panel for List {
    fn cursor(&self, pos: Point) -> MouseCursor {
        if self.row_at(pos).is_some() {
            MouseCursor::Hand
        } else {
            MouseCursor::Default
        }
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn opaque(&self) -> Option<Rect> {
        crate::panel::body(self.rect)
    }

    fn draw(&self, canvas: &mut PixmapMut<'_>, transform: Transform) {
        let inset = 0.5 / transform.sx;
        let outline = Rect::new(
            self.rect.x + inset,
            self.rect.y + inset,
            self.rect.w - 2.0 * inset,
            self.rect.h - 2.0 * inset,
        );
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        let path = rounded(outline, RADIUS);
        if let Some(path) = &path {
            paint.set_color(color(theme().background));
            canvas.fill_path(path, &paint, FillRule::Winding, transform, None);
        }

        let view = self.view();
        let text_w = self
            .bar()
            .map_or(view.w, |(track, _)| track.x - view.x - 1.0);
        let (ascent, descent) = self.lettering.face.line_metrics(TEXT_SIZE);
        let style = Style {
            face: &self.lettering.face,
            size: TEXT_SIZE,
            color: color(theme().ink),
        };
        let dim_style = Style {
            color: color(theme().dim_ink),
            ..style
        };
        let clip = [view.x, view.y, view.x + text_w, view.bottom()];
        let first = (self.scroll / ROW_H).floor() as usize;
        let mut text = self.lettering.text.borrow_mut();
        for (i, label) in self.items.iter().enumerate().skip(first) {
            let top = view.y + i as f32 * ROW_H - self.scroll;
            if top >= view.bottom() {
                break;
            }
            // The row's band, cut to the viewport.
            let band_top = top.max(view.y);
            let band = Rect::new(
                view.x,
                band_top,
                text_w,
                (top + ROW_H).min(view.bottom()) - band_top,
            );
            if self.selected == Some(i) {
                Self::fill(canvas, band, theme().selected, transform);
            } else if self.hover == Some(i) {
                Self::fill(canvas, band, theme().hover, transform);
            }
            let baseline = top + (ROW_H + ascent + descent) / 2.0;
            let dimmed = self.dimmed.get(i).copied().unwrap_or(false);
            let mut x = view.x + PAD_X;
            if let Some(glyph) = &self.glyph {
                let (gw, gh) = glyph_size(ROW_H);
                let at = Rect::new(x, top + (ROW_H - gh) / 2.0, gw, gh);
                // Only whole: a glyph isn't clipped to the view.
                if at.y >= view.y && at.bottom() <= view.bottom() {
                    let ink = if dimmed { theme().dim_ink } else { theme().ink };
                    glyph(canvas, at, i, ink, transform);
                }
                x += gw + 6.0;
            }
            text.draw_clipped(
                canvas,
                label,
                (x, baseline),
                if dimmed { &dim_style } else { &style },
                transform,
                Some(clip),
            );
        }
        drop(text);

        if let Some((track, thumb)) = self.bar() {
            Self::fill(canvas, track, theme().scroll_track, transform);
            Self::fill(canvas, thumb, theme().scroll_thumb, transform);
        }
        if let Some(path) = &path {
            paint.set_color(color(theme().border));
            let stroke = Stroke {
                width: 1.0 / transform.sx,
                ..Stroke::default()
            };
            canvas.stroke_path(path, &paint, &stroke, transform, None);
        }
    }

    fn input(&mut self, event: &RawKind, state: &mut EditorState) -> Reply {
        match *event {
            RawKind::Wheel { delta, .. } => {
                if self.set_scroll(self.scroll - delta.y * ROW_H) {
                    Reply::Handled
                } else {
                    Reply::Ignored
                }
            }
            RawKind::PointerMove { pos, .. } => {
                if let Some((y0, scroll0)) = self.dragging {
                    let (track, thumb) = self.bar().expect("dragging implies a bar");
                    let per = self.max_scroll() / (track.h - thumb.h).max(1.0);
                    self.set_scroll(scroll0 + (pos.y - y0) * per);
                    return Reply::Handled;
                }
                let hover = self.row_at(pos);
                if hover == self.hover {
                    return Reply::Ignored;
                }
                // Repaint the rows the hover left and entered.
                let rects = [self.hover, hover]
                    .into_iter()
                    .flatten()
                    .filter_map(|i| self.row_rect(i))
                    .collect();
                self.hover = hover;
                Reply::repaint(rects)
            }
            RawKind::PointerDown { pos, .. } => {
                if let Some((track, thumb)) = self.bar() {
                    if thumb.contains(pos) {
                        self.dragging = Some((pos.y, self.scroll));
                        return Reply::Handled;
                    }
                    if Rect::new(
                        track.x - 1.0,
                        self.view().y,
                        track.w + 2.0 * BAR_MARGIN,
                        self.view().h,
                    )
                    .contains(pos)
                    {
                        let page = self.view().h - ROW_H;
                        let dir = if pos.y < thumb.y { -1.0 } else { 1.0 };
                        self.set_scroll(self.scroll + dir * page);
                        return Reply::Handled;
                    }
                }
                self.pressed = self.row_at(pos);
                match self.pressed {
                    Some(i) => {
                        self.selected = Some(i);
                        Reply::Handled
                    }
                    None => Reply::Ignored,
                }
            }
            RawKind::PointerUp { pos, .. } => {
                if self.dragging.take().is_some() {
                    return Reply::Handled;
                }
                match self.pressed.take() {
                    Some(i) if self.row_at(pos) == Some(i) => (self.on_choose)(i, state),
                    _ => Reply::Ignored,
                }
            }
            RawKind::Key {
                key, pressed: true, ..
            } => self.key(key, state),
            _ => Reply::Ignored,
        }
    }
}
