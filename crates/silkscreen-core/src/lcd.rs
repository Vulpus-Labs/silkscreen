//! A low-resolution LCD: content drawn in a coarse grid of dots, presented
//! as lit and unlit dots behind glass ([design/faceplates.md]).
//!
//! An editor draws into an [`LcdBitmap`] in LCD pixels, whatever the zoom,
//! with its lines, rects and [`LcdFont`] text, then calls
//! [`LcdBitmap::draw`]. Nothing is resampled: every dot is a tile of whole
//! device pixels with sharp edges. Where the pitch times the scale isn't
//! whole, tiles are one pixel wider or narrower in turn, so the grid fills
//! its window at any zoom. The gap between dots is the same for
//! every dot; only a lit dot's width varies, by at most a pixel. At a whole
//! pitch every dot is the same tile.
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

use tiny_skia::{Color, PixmapMut, Transform};

use crate::geom::{Point, Rect};

/// The widest tile a dot may have, device px.
const MAX_PITCH: usize = 32;

/// A bitmap face. `silkscreen_build::BitmapFont` generates these from a
/// text source, and fails the build if a required glyph is missing.
#[derive(Clone, Copy, Debug)]
pub struct LcdFont {
    /// Glyph width in dots, at most 8.
    pub width: u8,
    /// Rows a glyph has.
    pub height: u8,
    /// Dots from one glyph's left edge to the next.
    pub advance: u8,
    /// Sorted by character. Each glyph is `height` rows, top first, with
    /// a row's leftmost dot in bit `width - 1`.
    pub glyphs: &'static [(char, &'static [u8])],
}

impl LcdFont {
    /// `c`'s rows, if the face has it.
    pub fn glyph(&self, c: char) -> Option<&'static [u8]> {
        let i = self.glyphs.binary_search_by_key(&c, |g| g.0).ok()?;
        Some(self.glyphs[i].1)
    }

    /// Dots `text` spans, trailing gap included.
    pub fn text_width(&self, text: &str) -> i32 {
        text.chars().count() as i32 * i32::from(self.advance)
    }
}

/// How an LCD looks. Colours are opaque; alpha is ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LcdStyle {
    /// Dot pitch in logical units.
    pub pitch: f32,
    /// The gap between dots, as a share of the pitch. It rounds to whole
    /// device pixels once a dot is three or more wide.
    pub gap: f32,
    /// The glass, seen in the gaps and around the grid.
    pub glass: Color,
    /// An unlit dot.
    pub off: Color,
    /// A lit dot.
    pub on: Color,
    /// How far each lit neighbour draws an unlit dot toward `on`, for a
    /// backlit LCD's bleed. 0 for a reflective one.
    pub halo: f32,
    /// White added at the top of the glass, fading out halfway down.
    pub sheen: f32,
}

impl Default for LcdStyle {
    fn default() -> Self {
        Self {
            pitch: 2.0,
            gap: 0.25,
            glass: Color::from_rgba8(0x14, 0x0c, 0x04, 0xff),
            off: Color::from_rgba8(0x26, 0x17, 0x0a, 0xff),
            on: Color::from_rgba8(0xff, 0xb6, 0x48, 0xff),
            halo: 0.0,
            sheen: 0.0,
        }
    }
}

/// Where a grid of dots sits, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    /// The grid's top left, on a whole pixel, so a crop (a whole-pixel
    /// translation) moves every dot by exactly that much.
    x: i32,
    y: i32,
    /// Device pixels from one dot's start to the next's, on average. Dot
    /// `i` starts [`edge`]`(i, pitch)` pixels from the grid's origin.
    pitch: f32,
}

/// Where dot `i` starts, in device pixels from the grid's origin, at
/// `pitch`. Whole numbers of pixels: dots are `floor(pitch)` or one more
/// wide, and `edge(n) - edge(0)` is `pitch × n` rounded.
fn edge(i: i32, pitch: f32) -> i32 {
    (i as f32 * pitch).round() as i32
}

/// The dot `offset` device pixels from the grid's origin falls in, at
/// `pitch`; negative before it.
fn dot_of(offset: i32, pitch: f32) -> i32 {
    let mut i = (offset as f32 / pitch).floor() as i32;
    // The float guess can be one off either way where an edge rounded.
    if edge(i + 1, pitch) <= offset {
        i += 1;
    } else if edge(i, pitch) > offset {
        i -= 1;
    }
    i
}

/// An LCD's content, one flag a dot. Drawing outside it does nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LcdBitmap {
    width: u16,
    height: u16,
    dots: Vec<bool>,
}

impl LcdBitmap {
    /// A blank LCD, `width` by `height` dots.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            dots: vec![false; usize::from(width) * usize::from(height)],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Unlight every dot.
    pub fn clear(&mut self) {
        self.dots.fill(false);
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let in_x = (0..i32::from(self.width)).contains(&x);
        let in_y = (0..i32::from(self.height)).contains(&y);
        (in_x && in_y).then(|| y as usize * usize::from(self.width) + x as usize)
    }

    /// Whether the dot at `(x, y)` is lit; false outside the bitmap.
    pub fn get(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.dots[i])
    }

    /// Light or unlight one dot.
    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if let Some(i) = self.index(x, y) {
            self.dots[i] = on;
        }
    }

    /// A one-dot line, ends included.
    pub fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), on: bool) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, on);
            if (x, y) == (x1, y1) {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Set every dot in the rect.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        for row in y..y + h {
            for col in x..x + w {
                self.set(col, row, on);
            }
        }
    }

    /// A one-dot outline.
    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        self.fill_rect(x, y, w, 1, on);
        self.fill_rect(x, y + h - 1, w, 1, on);
        self.fill_rect(x, y, 1, h, on);
        self.fill_rect(x + w - 1, y, 1, h, on);
    }

    /// Set `text`'s dots with `(x, y)` its top left, and return the x after
    /// it. A character the face lacks draws as a box; a build that requires
    /// it fails first (`silkscreen_build::BitmapFont`).
    pub fn text(&mut self, font: &LcdFont, x: i32, y: i32, text: &str, on: bool) -> i32 {
        let (w, h) = (i32::from(font.width), i32::from(font.height));
        let mut pen = x;
        for c in text.chars() {
            match font.glyph(c) {
                Some(rows) => {
                    for (j, row) in rows.iter().enumerate() {
                        for i in 0..w {
                            if row >> (w - 1 - i) & 1 == 1 {
                                self.set(pen + i, y + j as i32, on);
                            }
                        }
                    }
                }
                None => self.stroke_rect(pen, y, w, h, on),
            }
            pen += i32::from(font.advance);
        }
        pen
    }

    /// The grid for logical `rect`, device edges rounded so that a crop
    /// (a whole-pixel translation) moves it by exactly that much.
    fn grid(&self, rect: Rect, style: &LcdStyle, transform: Transform) -> Grid {
        let s = transform.sx;
        let x0 = (rect.x * s + transform.tx).round() as i32;
        let y0 = (rect.y * s + transform.ty).round() as i32;
        let x1 = ((rect.x + rect.w) * s + transform.tx).round() as i32;
        let y1 = ((rect.y + rect.h) * s + transform.ty).round() as i32;
        let (w, h) = (i32::from(self.width).max(1), i32::from(self.height).max(1));
        // The style's pitch, no bigger than fits the window (the window is
        // whole pixels, the content is not) and no smaller than a pixel.
        let fit = ((x1 - x0) as f32 / w as f32).min((y1 - y0) as f32 / h as f32);
        let pitch = (style.pitch * s)
            .min(fit)
            .clamp(1.0, MAX_PITCH as f32 - 1.0);
        Grid {
            x: x0 + (x1 - x0 - edge(w, pitch)) / 2,
            y: y0 + (y1 - y0 - edge(h, pitch)) / 2,
            pitch,
        }
    }

    /// The dot under logical `pos` for an LCD drawn over `rect` at `scale`,
    /// for hit testing a handle. It may lie outside the bitmap, so a drag
    /// past the edge still reads.
    pub fn dot_at(&self, rect: Rect, style: &LcdStyle, scale: f32, pos: Point) -> (i32, i32) {
        let grid = self.grid(rect, style, Transform::from_scale(scale, scale));
        (
            dot_of((pos.x * scale).floor() as i32 - grid.x, grid.pitch),
            dot_of((pos.y * scale).floor() as i32 - grid.y, grid.pitch),
        )
    }

    /// A dot's size in logical units for an LCD drawn over `rect` at `scale`,
    /// on average: the style's pitch, unless the window is too small for it.
    /// A drag divides by it to move in dots.
    pub fn pitch_at(&self, rect: Rect, style: &LcdStyle, scale: f32) -> f32 {
        let grid = self.grid(rect, style, Transform::from_scale(scale, scale));
        grid.pitch / scale
    }

    /// Paint the LCD over logical `rect`: glass, then every dot. Opaque,
    /// inside `rect`. Allocates nothing.
    pub fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        style: &LcdStyle,
    ) {
        let s = transform.sx;
        let x0 = ((rect.x * s + transform.tx).round() as i32).max(0);
        let y0 = ((rect.y * s + transform.ty).round() as i32).max(0);
        let top = (rect.y * s + transform.ty).round() as i32;
        let (cw, ch) = (canvas.width() as i32, canvas.height() as i32);
        let x1 = (((rect.x + rect.w) * s + transform.tx).round() as i32).min(cw);
        let y1 = (((rect.y + rect.h) * s + transform.ty).round() as i32).min(ch);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let grid = self.grid(rect, style, transform);
        // Tiles are `narrow` or `narrow + 1` pixels; one coverage table each.
        let gap = gap_px(grid.pitch, style.gap);
        let narrow = (grid.pitch.floor() as usize).clamp(1, MAX_PITCH - 1);
        let covers = [cover(narrow, gap), cover(narrow + 1, gap)];
        let tile = |width: i32| &covers[usize::from(width as usize > narrow)];
        let (glass, off, on) = (rgb(style.glass), rgb(style.off), rgb(style.on));
        let halo = (style.halo.clamp(0.0, 1.0) * 256.0) as u32;
        let sheen_span = ((rect.h * s) / 2.0).max(1.0);
        let (w, h) = (i32::from(self.width), i32::from(self.height));
        let stride = canvas.width() as usize * 4;
        let data = canvas.data_mut();

        for y in y0..y1 {
            let sheen = (style.sheen * (1.0 - (y - top) as f32 / sheen_span)).clamp(0.0, 1.0);
            let sheen = (sheen * 256.0) as u32;
            let row = y - grid.y;
            let dot_y = dot_of(row, grid.pitch);
            let on_row = (0..h).contains(&dot_y);
            let top_of_dot = edge(dot_y, grid.pitch);
            let tall = edge(dot_y + 1, grid.pitch) - top_of_dot;
            let cover_y = tile(tall)[(row - top_of_dot) as usize];
            let line = &mut data[y as usize * stride..][..stride];
            // The dot under `x`, walked rather than found each pixel.
            let mut dot_x = dot_of(x0 - grid.x, grid.pitch);
            let mut start = edge(dot_x, grid.pitch);
            let mut end = edge(dot_x + 1, grid.pitch);
            let mut colour = if on_row && (0..w).contains(&dot_x) {
                self.dot_colour(dot_x, dot_y, off, on, halo)
            } else {
                off
            };
            for x in x0..x1 {
                let col = x - grid.x;
                if col >= end {
                    dot_x += 1;
                    start = end;
                    end = edge(dot_x + 1, grid.pitch);
                    if on_row && (0..w).contains(&dot_x) {
                        colour = self.dot_colour(dot_x, dot_y, off, on, halo);
                    }
                }
                let share = if on_row && (0..w).contains(&dot_x) {
                    (tile(end - start)[(col - start) as usize] * cover_y) >> 8
                } else {
                    0
                };
                let px = mix(glass, colour, share);
                let px = mix(px, [255; 3], sheen);
                let at = x as usize * 4;
                line[at..at + 4].copy_from_slice(&[px[0] as u8, px[1] as u8, px[2] as u8, 255]);
            }
        }
    }

    fn dot_colour(&self, x: i32, y: i32, off: [u32; 3], on: [u32; 3], halo: u32) -> [u32; 3] {
        if self.get(x, y) {
            return on;
        }
        if halo == 0 {
            return off;
        }
        let near = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
            .iter()
            .filter(|&&(a, b)| self.get(a, b))
            .count() as u32;
        mix(off, on, (near * halo).min(256))
    }
}

/// The gap between dots in device pixels, for every dot of a grid at
/// `pitch`: `gap` (a share of the pitch), rounded to whole pixels once a
/// dot is three or more wide.
fn gap_px(pitch: f32, gap: f32) -> f32 {
    let gap = pitch * gap.clamp(0.0, 0.9);
    if pitch >= 3.0 {
        gap.round().max(1.0)
    } else {
        gap
    }
}

/// One axis of a tile `width` pixels wide with a `gap`-pixel gap: how much
/// of each device pixel the dot covers, out of 256.
fn cover(width: usize, gap: f32) -> [u32; MAX_PITCH] {
    let width = width.clamp(1, MAX_PITCH);
    let lit = (width as f32 - gap).max(0.5);
    let mut table = [0; MAX_PITCH];
    for (i, c) in table.iter_mut().take(width).enumerate() {
        *c = ((lit - i as f32).clamp(0.0, 1.0) * 256.0) as u32;
    }
    table
}

fn rgb(c: Color) -> [u32; 3] {
    let c = c.to_color_u8();
    [c.red().into(), c.green().into(), c.blue().into()]
}

/// `a` toward `b` by `t`/256.
fn mix(a: [u32; 3], b: [u32; 3], t: u32) -> [u32; 3] {
    [0, 1, 2].map(|i| (a[i] * (256 - t) + b[i] * t) >> 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    static FONT: LcdFont = LcdFont {
        width: 3,
        height: 3,
        advance: 4,
        glyphs: &[('A', &[0b111, 0b101, 0b111])],
    };

    fn render(b: &LcdBitmap, rect: Rect, scale: f32, style: &LcdStyle) -> Pixmap {
        let (w, h) = (
            ((rect.x + rect.w) * scale).ceil() as u32,
            ((rect.y + rect.h) * scale).ceil() as u32,
        );
        let mut p = Pixmap::new(w, h).unwrap();
        b.draw(
            &mut p.as_mut(),
            rect,
            Transform::from_scale(scale, scale),
            style,
        );
        p
    }

    fn at(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = p.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    fn style() -> LcdStyle {
        LcdStyle {
            pitch: 2.0,
            gap: 0.25,
            glass: Color::from_rgba8(0, 0, 0, 255),
            off: Color::from_rgba8(0, 0, 80, 255),
            on: Color::from_rgba8(255, 255, 255, 255),
            halo: 0.0,
            sheen: 0.0,
        }
    }

    #[test]
    fn at_a_whole_pitch_every_dot_is_the_same_tile() {
        let mut b = LcdBitmap::new(8, 6);
        b.fill_rect(0, 0, 8, 6, true);
        let rect = Rect::new(3.0, 5.0, 16.0, 12.0);
        for scale in [1.0, 1.5, 2.0, 2.5, 3.0, 4.0] {
            let p = render(&b, rect, scale, &style());
            let grid = b.grid(rect, &style(), Transform::from_scale(scale, scale));
            let pitch = grid.pitch as i32;
            assert_eq!(pitch as f32, grid.pitch, "at {scale}");
            let tile = |dx: i32, dy: i32, i: i32, j: i32| {
                at(
                    &p,
                    (grid.x + dx * pitch + i) as u32,
                    (grid.y + dy * pitch + j) as u32,
                )
            };
            for (dx, dy) in [(0, 0), (7, 0), (3, 5), (7, 5)] {
                for j in 0..pitch {
                    for i in 0..pitch {
                        assert_eq!(tile(dx, dy, i, j), tile(0, 0, i, j), "at {scale}");
                    }
                }
            }
        }
    }

    /// The grid fills its window at any zoom, its tiles a pixel apart
    /// at most, rather than rounding the pitch down and shrinking.
    #[test]
    fn the_grid_fills_its_window_at_a_fractional_zoom() {
        let b = LcdBitmap::new(8, 6);
        let rect = Rect::new(3.0, 5.0, 16.0, 12.0);
        for scale in [1.0_f32, 1.25, 1.39, 1.5, 1.75, 2.0, 2.78, 3.0] {
            let g = b.grid(rect, &style(), Transform::from_scale(scale, scale));
            let (x0, x1) = ((3.0 * scale).round() as i32, (19.0 * scale).round() as i32);
            let (y0, y1) = ((5.0 * scale).round() as i32, (17.0 * scale).round() as i32);
            // Inside the window, and within a pixel of filling it.
            assert!(g.x >= x0 && g.x + edge(8, g.pitch) <= x1, "at {scale}");
            assert!(g.y >= y0 && g.y + edge(6, g.pitch) <= y1, "at {scale}");
            assert!(
                x1 - (g.x + edge(8, g.pitch)) - (g.x - x0) <= 1,
                "at {scale}"
            );
            assert!(
                y1 - (g.y + edge(6, g.pitch)) - (g.y - y0) <= 1,
                "at {scale}"
            );
            let widths: Vec<i32> = (0..8)
                .map(|i| edge(i + 1, g.pitch) - edge(i, g.pitch))
                .collect();
            let (lo, hi) = (*widths.iter().min().unwrap(), *widths.iter().max().unwrap());
            assert!(hi - lo <= 1, "at {scale}: {widths:?}");
        }
    }

    #[test]
    fn dot_of_inverts_edge() {
        for pitch in [1.0_f32, 2.0, 2.5, 2.78, 3.47, 5.56] {
            for i in -3..20 {
                for px in edge(i, pitch)..edge(i + 1, pitch) {
                    assert_eq!(dot_of(px, pitch), i, "pitch {pitch} px {px}");
                }
            }
        }
    }

    #[test]
    fn every_dot_has_the_same_gap() {
        // At pitch 2.5 tiles are 2 and 3 wide; the gap is the grid's, 0.625.
        let gap = gap_px(2.5, 0.25);
        assert_eq!(gap, 0.625);
        assert_eq!(cover(2, gap)[..2], [256, 96]);
        assert_eq!(cover(3, gap)[..3], [256, 256, 96]);
        // From pitch 3 the gap is whole pixels.
        assert_eq!(gap_px(4.0, 0.25), 1.0);
        assert_eq!(cover(4, 1.0)[..4], [256, 256, 256, 0]);
        assert_eq!(cover(3, gap_px(3.0, 0.25))[..3], [256, 256, 0]);
    }

    #[test]
    fn a_crop_moves_pixels_by_exactly_the_crop() {
        // The compositor draws a damage rect through a whole-pixel offset.
        let mut b = LcdBitmap::new(8, 6);
        b.line((0, 0), (7, 5), true);
        let rect = Rect::new(3.0, 5.0, 16.0, 12.0);
        let whole = render(&b, rect, 1.25, &style());
        let mut part = Pixmap::new(10, 8).unwrap();
        let (ox, oy) = (6, 9);
        b.draw(
            &mut part.as_mut(),
            rect,
            Transform::from_row(1.25, 0.0, 0.0, 1.25, -(ox as f32), -(oy as f32)),
            &style(),
        );
        for y in 0..8 {
            for x in 0..10 {
                assert_eq!(at(&part, x, y), at(&whole, x + ox, y + oy));
            }
        }
    }

    #[test]
    fn draws_only_inside_its_rect() {
        let b = LcdBitmap::new(4, 4);
        let p = render(&b, Rect::new(2.0, 2.0, 8.0, 8.0), 1.0, &style());
        assert_eq!(at(&p, 1, 5), [0, 0, 0, 0]);
        assert_eq!(at(&p, 5, 1), [0, 0, 0, 0]);
        assert_eq!(at(&p, 2, 2)[3], 255);
        assert_eq!(at(&p, 9, 9)[3], 255);
    }

    #[test]
    fn text_sets_a_glyphs_dots_and_boxes_what_is_missing() {
        let mut b = LcdBitmap::new(8, 3);
        assert_eq!(b.text(&FONT, 0, 0, "A?", true), 8);
        assert!(b.get(0, 0) && b.get(2, 0) && !b.get(1, 1));
        assert!(b.get(4, 0) && b.get(4, 2) && !b.get(5, 1), "a box");
        assert_eq!(FONT.text_width("AA"), 8);
    }

    #[test]
    fn line_reaches_both_ends_and_clips() {
        let mut b = LcdBitmap::new(5, 5);
        b.line((-3, -3), (7, 7), true);
        assert!((0..5).all(|i| b.get(i, i)));
        b.clear();
        assert!(!b.get(2, 2));
        b.line((4, 0), (0, 1), true);
        assert!(b.get(4, 0) && b.get(0, 1));
    }

    #[test]
    fn a_dot_is_the_styles_pitch_in_logical_units() {
        let b = LcdBitmap::new(8, 6);
        let rect = Rect::new(0.0, 0.0, 16.0, 12.0);
        assert_eq!(b.pitch_at(rect, &style(), 2.0), 2.0);
        assert_eq!(b.pitch_at(rect, &style(), 1.25), 2.0);
        // A window too small for the style's pitch shrinks it.
        assert_eq!(
            b.pitch_at(Rect::new(0.0, 0.0, 12.0, 12.0), &style(), 1.0),
            1.5
        );
    }

    #[test]
    fn dot_at_follows_the_grid_at_a_fractional_pitch() {
        let b = LcdBitmap::new(8, 6);
        let rect = Rect::new(0.0, 0.0, 16.0, 12.0);
        let s = style();
        assert_eq!(b.dot_at(rect, &s, 2.0, Point::new(4.1, 2.1)), (2, 1));
        // At 1.25 the pitch is 2.5 device px and the grid fills all 20:
        // dots start at 0, 3, 5, 8, 10 … (half rounds away from zero).
        assert_eq!(b.dot_at(rect, &s, 1.25, Point::new(2.0, 1.5)), (0, 0));
        assert_eq!(b.dot_at(rect, &s, 1.25, Point::new(2.4, 0.0)).0, 1);
        assert_eq!(b.dot_at(rect, &s, 1.25, Point::new(15.9, 0.0)).0, 7);
        assert_eq!(b.dot_at(rect, &s, 1.25, Point::new(-0.1, 0.0)).0, -1);
    }

    #[test]
    fn halo_lifts_only_the_unlit_neighbours_of_a_lit_dot() {
        let mut b = LcdBitmap::new(5, 1);
        b.set(2, 0, true);
        let (off, on) = ([0, 0, 80], [255, 255, 255]);
        assert_eq!(b.dot_colour(1, 0, off, on, 64), mix(off, on, 64));
        assert_eq!(b.dot_colour(0, 0, off, on, 64), off);
        assert_eq!(b.dot_colour(2, 0, off, on, 64), on);
    }
}
