//! The LCD canvas mode (design/faceplates.md) at the zooms a faceplate shows.

use silkscreen::core::Rect;
use silkscreen::core::lcd::{LcdBitmap, LcdFont, LcdStyle};
use silkscreen::core::tiny_skia::{Color, Pixmap, Transform};
use silkscreen_harness::check_snapshot;

fn font() -> LcdFont {
    let f = silkscreen_build::BitmapFont::new(silkscreen_build::LCD_FONT)
        .load()
        .unwrap();
    let glyphs: Vec<(char, &'static [u8])> = f
        .glyphs
        .into_iter()
        .map(|(c, rows)| (c, &*rows.leak()))
        .collect();
    LcdFont {
        width: f.width,
        height: f.height,
        advance: f.advance,
        glyphs: glyphs.leak(),
    }
}

fn content(font: &LcdFont) -> LcdBitmap {
    let mut b = LcdBitmap::new(150, 48);
    b.text(font, 1, 0, "OP 3 Envelope", true);
    b.text(font, 1, 40, "440.0 Hz -12 dB 180\u{b0} 1/4", true);
    let curve = [(2, 34), (18, 11), (50, 20), (108, 25), (146, 34)];
    for w in curve.windows(2) {
        b.line(w[0], w[1], true);
    }
    for &(x, y) in &curve[1..] {
        b.fill_rect(x - 1, y - 1, 3, 3, true);
    }
    b
}

fn styles() -> [(&'static str, LcdStyle); 2] {
    [
        (
            "amber",
            LcdStyle {
                halo: 0.08,
                sheen: 0.05,
                ..LcdStyle::default()
            },
        ),
        (
            "stn",
            LcdStyle {
                glass: Color::from_rgba8(0xa8, 0xb4, 0x8c, 0xff),
                off: Color::from_rgba8(0x9b, 0xa7, 0x81, 0xff),
                on: Color::from_rgba8(0x20, 0x28, 0x1e, 0xff),
                ..LcdStyle::default()
            },
        ),
    ]
}

#[test]
fn an_lcd_is_crisp_at_every_zoom() {
    let font = font();
    let bitmap = content(&font);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let rect = Rect::new(4.0, 4.0, 308.0, 104.0);
    for (name, style) in styles() {
        for (tag, scale) in [("1x", 1.0), ("1_25x", 1.25), ("1_5x", 1.5), ("2x", 2.0)] {
            let (w, h) = (316.0 * scale, 112.0 * scale);
            let mut p = Pixmap::new(w as u32, h as u32).unwrap();
            p.fill(Color::from_rgba8(0x30, 0x30, 0x30, 0xff));
            bitmap.draw(
                &mut p.as_mut(),
                rect,
                Transform::from_scale(scale, scale),
                &style,
            );
            if let Err(e) = check_snapshot(&p, &dir, &format!("lcd_{name}@{tag}")) {
                panic!("{e}");
            }
        }
    }
}
