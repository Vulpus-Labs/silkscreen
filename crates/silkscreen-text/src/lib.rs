//! Runtime text for silkscreen ([design/overlays-and-text.md]): tooltips, value readouts,
//! menus, preset names. Faceplate lettering stays in the SVG.
//!
//! A [`Face`] is a TrueType font embedded in the binary, rasterised at run
//! time by fontdue. [`Text`] caches each glyph's coverage per face and
//! physical pixel size, lays strings out with the face's kerning, and
//! blends them onto a canvas through the same logical → physical
//! [`Transform`] widgets draw with. Glyphs land on whole physical pixels
//! and baselines on whole rows, so text stays crisp at every zoom step.
//!
//! Builds for `wasm32-unknown-unknown`: no threads, no filesystem.
//!
//! [design/overlays-and-text.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/overlays-and-text.md

use std::collections::HashMap;

use tiny_skia::{Color, PixmapMut, PremultipliedColorU8, Transform};

/// Jost Medium (OFL 1.1, this crate's `fonts/`): the default runtime face.
pub static JOST_MEDIUM: &[u8] = include_bytes!("../fonts/Jost-500-Medium.ttf");

/// The folder [`JOST_MEDIUM`] comes from, with its licence, on the machine
/// that built this crate: for build scripts to letter faceplates in the
/// same face (`silkscreen_build::TEXT_FONT_DIR`).
pub const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts");

/// A parsed font. Cheap to share by reference; parse each once.
pub struct Face {
    font: fontdue::Font,
    id: u32,
}

impl Face {
    /// Parse `bytes` (TrueType or OpenType). `id` tells faces apart in a
    /// [`Text`] cache; give each face its own.
    ///
    /// # Panics
    /// If `bytes` isn't a font fontdue can read: faces are embedded, so
    /// that's a build mistake.
    pub fn new(bytes: &[u8], id: u32) -> Self {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .unwrap_or_else(|e| panic!("embedded font {id} won't parse: {e}"));
        Self { font, id }
    }

    /// The face's ascent and descent (descent negative) at `size`, in the
    /// same units as `size`.
    pub fn line_metrics(&self, size: f32) -> (f32, f32) {
        self.font
            .horizontal_line_metrics(size)
            .map_or((size * 0.8, -size * 0.2), |m| (m.ascent, m.descent))
    }
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// One rasterised glyph: where it sits relative to the pen and baseline,
/// and its coverage, row-major from the top.
struct Glyph {
    xmin: i32,
    ymin: i32,
    width: usize,
    height: usize,
    advance: f32,
    coverage: Vec<u8>,
}

/// Glyph cache and text drawing. Keep one per view and reuse it: each
/// glyph is rasterised once per face and size.
#[derive(Default)]
pub struct Text {
    glyphs: HashMap<(u32, char, u32), Glyph>,
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Text")
            .field("cached", &self.glyphs.len())
            .finish()
    }
}

/// Physical sizes are cached in quarter pixels.
fn size_key(px: f32) -> u32 {
    (px * 4.0).round() as u32
}

impl Text {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of glyphs rasterised so far.
    pub fn cached(&self) -> usize {
        self.glyphs.len()
    }

    fn glyph(&mut self, face: &Face, c: char, px: f32) -> &Glyph {
        let key = (face.id, c, size_key(px));
        self.glyphs.entry(key).or_insert_with(|| {
            let (m, coverage) = face.font.rasterize(c, key.2 as f32 / 4.0);
            Glyph {
                xmin: m.xmin,
                ymin: m.ymin,
                width: m.width,
                height: m.height,
                advance: m.advance_width,
                coverage,
            }
        })
    }

    /// Width of `text` at `size` logical units, as drawn at `scale`
    /// physical pixels per unit (glyph positions are rounded to pixels, so
    /// width depends a little on scale).
    pub fn width(&mut self, face: &Face, text: &str, size: f32, scale: f32) -> f32 {
        let px = size * scale;
        let mut pen = 0.0;
        let mut prev = None;
        for c in text.chars() {
            if let Some(p) = prev {
                pen += face.font.horizontal_kern(p, c, px).unwrap_or(0.0);
            }
            pen += self.glyph(face, c, px).advance;
            prev = Some(c);
        }
        pen / scale
    }

    /// Draw `text` with its baseline starting at logical `origin`, in
    /// `style`, through `transform` (logical → canvas pixels, scale and
    /// offset applied, no rotation or skew). Returns the pen's logical x
    /// after the last glyph.
    pub fn draw(
        &mut self,
        canvas: &mut PixmapMut<'_>,
        text: &str,
        origin: (f32, f32),
        style: &Style<'_>,
        transform: Transform,
    ) -> f32 {
        self.draw_clipped(canvas, text, origin, style, transform, None)
    }

    /// [`draw`](Self::draw), keeping ink inside the logical rect `clip`
    /// (`[left, top, right, bottom]`), e.g. a scrolling list's viewport.
    pub fn draw_clipped(
        &mut self,
        canvas: &mut PixmapMut<'_>,
        text: &str,
        origin: (f32, f32),
        style: &Style<'_>,
        transform: Transform,
        clip: Option<[f32; 4]>,
    ) -> f32 {
        let face = style.face;
        let scale = transform.sx;
        let px = style.size * scale;
        let mut pen = origin.0 * scale + transform.tx;
        let baseline = (origin.1 * scale + transform.ty).round() as i32;
        let src = style.color.premultiply().to_color_u8();
        let (width, height) = (canvas.width() as i32, canvas.height() as i32);
        // The physical pixels ink may land on: the canvas, within `clip`.
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, width, height);
        if let Some([left, top, right, bottom]) = clip {
            let to_x = |v: f32| (v * scale + transform.tx).round() as i32;
            let to_y = |v: f32| (v * transform.sy + transform.ty).round() as i32;
            x0 = x0.max(to_x(left));
            x1 = x1.min(to_x(right));
            y0 = y0.max(to_y(top));
            y1 = y1.min(to_y(bottom));
        }
        // A line whose band (a size above the baseline, a size below)
        // misses the rows ink may land on draws nothing: it only advances.
        let reach = px.ceil() as i32;
        let visible = baseline + reach > y0 && baseline - reach < y1;
        let mut prev = None;
        for ch in text.chars() {
            if let Some(p) = prev {
                pen += face.font.horizontal_kern(p, ch, px).unwrap_or(0.0);
            }
            let glyph = self.glyph(face, ch, px);
            if !visible {
                pen += glyph.advance;
                prev = Some(ch);
                continue;
            }
            let left = pen.round() as i32 + glyph.xmin;
            let top = baseline - glyph.height as i32 - glyph.ymin;
            let pixels = canvas.pixels_mut();
            for row in 0..glyph.height {
                let py = top + row as i32;
                if !(y0..y1).contains(&py) {
                    continue;
                }
                for col in 0..glyph.width {
                    let px_x = left + col as i32;
                    if !(x0..x1).contains(&px_x) {
                        continue;
                    }
                    let cover = glyph.coverage[row * glyph.width + col];
                    if cover == 0 {
                        continue;
                    }
                    let i = (py * width + px_x) as usize;
                    pixels[i] = over(src, cover, pixels[i]);
                }
            }
            pen += glyph.advance;
            prev = Some(ch);
        }
        (pen - transform.tx) / scale
    }
}

/// How text looks: its face, size in logical units, and colour.
#[derive(Clone, Copy, Debug)]
pub struct Style<'a> {
    pub face: &'a Face,
    pub size: f32,
    pub color: Color,
}

/// `src` at `coverage` over `dst`, premultiplied.
fn over(
    src: PremultipliedColorU8,
    coverage: u8,
    dst: PremultipliedColorU8,
) -> PremultipliedColorU8 {
    let k = u32::from(coverage);
    let mul = |v: u8| (u32::from(v) * k + 127) / 255;
    let (sr, sg, sb, sa) = (
        mul(src.red()),
        mul(src.green()),
        mul(src.blue()),
        mul(src.alpha()),
    );
    let inv = 255 - sa;
    let blend = |s: u32, d: u8| (s + (u32::from(d) * inv + 127) / 255).min(255) as u8;
    let a = blend(sa, dst.alpha());
    PremultipliedColorU8::from_rgba(
        blend(sr, dst.red()).min(a),
        blend(sg, dst.green()).min(a),
        blend(sb, dst.blue()).min(a),
        a,
    )
    .unwrap_or(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    #[test]
    fn jost_parses_and_measures() {
        let face = Face::new(JOST_MEDIUM, 0);
        let mut text = Text::new();
        let w1 = text.width(&face, "Cutoff 1.2 kHz", 11.0, 1.0);
        let w2 = text.width(&face, "Cutoff 1.2 kHz", 11.0, 2.0);
        assert!(w1 > 50.0 && w1 < 90.0, "{w1}");
        assert!((w1 - w2).abs() < 2.0, "{w1} vs {w2}");
        let (ascent, descent) = face.line_metrics(11.0);
        assert!(ascent > 7.0 && descent < 0.0);
    }

    #[test]
    fn draws_inside_the_canvas_and_caches_glyphs() {
        let face = Face::new(JOST_MEDIUM, 0);
        let mut text = Text::new();
        let mut pm = Pixmap::new(80, 20).unwrap();
        let style = Style {
            face: &face,
            size: 12.0,
            color: Color::WHITE,
        };
        let end = text.draw(
            &mut pm.as_mut(),
            "Hello",
            (2.0, 14.0),
            &style,
            Transform::identity(),
        );
        assert!(end > 20.0);
        assert!(pm.pixels().iter().any(|p| p.alpha() > 200), "ink");
        assert_eq!(text.cached(), 4, "H e l o");
        // Off the canvas: clipped, no panic.
        let big = Style {
            size: 30.0,
            ..style
        };
        text.draw(
            &mut pm.as_mut(),
            "Hello",
            (-30.0, 40.0),
            &big,
            Transform::identity(),
        );
    }

    #[test]
    fn clipping_keeps_ink_inside() {
        let face = Face::new(JOST_MEDIUM, 0);
        let mut text = Text::new();
        let mut pm = Pixmap::new(80, 30).unwrap();
        let style = Style {
            face: &face,
            size: 20.0,
            color: Color::WHITE,
        };
        let clip = [0.0, 10.0, 80.0, 20.0];
        text.draw_clipped(
            &mut pm.as_mut(),
            "Hy",
            (2.0, 22.0),
            &style,
            Transform::identity(),
            Some(clip),
        );
        for (i, p) in pm.pixels().iter().enumerate() {
            let y = i / 80;
            assert!(p.alpha() == 0 || (10..20).contains(&y), "ink at row {y}");
        }
        assert!(pm.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn coverage_blends_over_premultiplied() {
        let red = Color::from_rgba8(255, 0, 0, 255)
            .premultiply()
            .to_color_u8();
        let black = PremultipliedColorU8::from_rgba(0, 0, 0, 255).unwrap();
        let half = over(red, 128, black);
        assert_eq!((half.red(), half.alpha()), (128, 255));
        let clear = PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
        assert_eq!(over(red, 255, clear).alpha(), 255);
    }
}
