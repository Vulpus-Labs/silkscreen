//! Frame buffer → screen ([design/rendering.md]): our own `CALayer` on macOS,
//! softbuffer elsewhere.
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

#[cfg(target_os = "macos")]
mod calayer;
#[cfg(not(target_os = "macos"))]
mod soft;

#[cfg(target_os = "macos")]
pub(crate) use calayer::Presenter;
#[cfg(not(target_os = "macos"))]
pub(crate) use soft::Presenter;

pub(crate) type PresentError = Box<dyn std::error::Error>;

use silkscreen::IRect;
use silkscreen::core::tiny_skia::Pixmap;

/// Convert `rect` of an opaque RGBA8 frame into softbuffer's `0RGB` `u32`s.
/// `dst` is `frame.width()` pixels wide.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn convert_rect(frame: &Pixmap, rect: IRect, dst: &mut [u32]) {
    let w = frame.width() as usize;
    let src = frame.data();
    for row in rect.y as usize..rect.bottom() as usize {
        for col in rect.x as usize..rect.right() as usize {
            let i = row * w + col;
            let p = &src[i * 4..i * 4 + 4];
            dst[i] = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silkscreen::core::tiny_skia::Color;

    #[test]
    fn converts_only_the_rect_to_0rgb() {
        let mut frame = Pixmap::new(4, 3).unwrap();
        frame.fill(Color::from_rgba8(0x12, 0x34, 0x56, 0xff));
        let mut dst = vec![0xdead_beef_u32; 12];
        convert_rect(&frame, IRect::new(1, 1, 2, 1), &mut dst);
        for (i, px) in dst.iter().enumerate() {
            let expected = if i == 5 || i == 6 {
                0x0012_3456
            } else {
                0xdead_beef
            };
            assert_eq!(*px, expected, "pixel {i}");
        }
    }
}
