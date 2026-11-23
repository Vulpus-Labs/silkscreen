//! A tiny 3×5 bitmap font for diagnostic labels (version badges,
//! counters). Not for faceplate text, which is pre-rendered from the SVG,
//! nor for user-facing runtime text, which `silkscreen-text` draws
//! (design/overlays-and-text.md).
//!
//! Covers `0-9`, `a-z` (drawn as small capitals), space, and `. : + - /`.
//! Anything else draws as a blank.

use tiny_skia::{Color, Paint, PixmapMut, Rect as SkRect, Transform};

/// Glyph rows, top to bottom, 3 bits each (bit 2 = left column).
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_lowercase() {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'a' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'b' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'c' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'd' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'e' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'f' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'g' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'h' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'i' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'j' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'k' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'l' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'm' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'n' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'o' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'p' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'r' => [0b110, 0b101, 0b110, 0b101, 0b101],
        's' => [0b011, 0b100, 0b010, 0b001, 0b110],
        't' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'u' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'v' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'w' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'x' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        _ => [0; 5],
    }
}

/// Glyph advance in font pixels (3 wide + 1 gap).
const ADVANCE: f32 = 4.0;
/// Glyph height in font pixels.
pub const HEIGHT: f32 = 5.0;

/// Width of `text` in logical units with font pixels `dot` units square.
pub fn width(text: &str, dot: f32) -> f32 {
    let n = text.chars().count() as f32;
    if n == 0.0 {
        0.0
    } else {
        (n * ADVANCE - 1.0) * dot
    }
}

/// Draw `text` with its top-left at logical `(x, y)`, each font pixel a
/// `dot`-unit square, through `transform` (logical → canvas pixels).
pub fn draw(
    canvas: &mut PixmapMut<'_>,
    text: &str,
    x: f32,
    y: f32,
    dot: f32,
    color: Color,
    transform: Transform,
) {
    let mut paint = Paint {
        anti_alias: false,
        ..Paint::default()
    };
    paint.set_color(color);
    for (i, c) in text.chars().enumerate() {
        let gx = x + i as f32 * ADVANCE * dot;
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0
                    && let Some(r) =
                        SkRect::from_xywh(gx + col as f32 * dot, y + row as f32 * dot, dot, dot)
                {
                    canvas.fill_rect(r, &paint, transform, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    #[test]
    fn width_counts_advances_without_a_trailing_gap() {
        assert_eq!(width("", 1.0), 0.0);
        assert_eq!(width("1", 1.0), 3.0);
        assert_eq!(width("10", 2.0), 14.0);
    }

    #[test]
    fn draws_the_glyph_pixels() {
        let mut p = Pixmap::new(3, 5).unwrap();
        draw(
            &mut p.as_mut(),
            "1",
            0.0,
            0.0,
            1.0,
            Color::WHITE,
            Transform::identity(),
        );
        let lit: Vec<bool> = p.pixels().iter().map(|c| c.alpha() == 255).collect();
        #[rustfmt::skip]
        let expected = [
            false, true,  false,
            true,  true,  false,
            false, true,  false,
            false, true,  false,
            true,  true,  true,
        ];
        assert_eq!(lit, expected);
    }
}
