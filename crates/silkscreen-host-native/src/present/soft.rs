//! Windows and Linux: present through softbuffer, damaged rects only.

use std::num::NonZeroU32;

use baseview::PlatformHandle;
use silkscreen::IRect;
use silkscreen::core::tiny_skia::Pixmap;
use softbuffer::{Context, Rect, Surface};

use super::{PresentError, convert_rect};

pub(crate) struct Presenter {
    surface: Surface<PlatformHandle, PlatformHandle>,
}

impl Presenter {
    pub fn new(handle: &PlatformHandle) -> Result<Self, PresentError> {
        let context = Context::new(handle.clone()).map_err(|e| e.to_string())?;
        let surface = Surface::new(&context, handle.clone()).map_err(|e| e.to_string())?;
        Ok(Self { surface })
    }

    pub fn resize(&mut self, w: u32, h: u32, _display_scale: f64) -> Result<(), PresentError> {
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| e.to_string().into())
    }

    /// Present `damage`, or the whole frame if `full`, if the buffer's old
    /// contents are gone, or if there is no damage (the OS asked for a
    /// redraw). `exposed` means the OS lost what was on screen (the window
    /// was shown, or uncovered): only `damage` is converted, but the whole
    /// buffer goes to the window. Returns `true` if the whole frame was
    /// converted.
    pub fn present(
        &mut self,
        frame: &Pixmap,
        damage: &[IRect],
        full: bool,
        exposed: bool,
    ) -> Result<bool, PresentError> {
        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        if full || buffer.age() == 0 || damage.is_empty() {
            let (w, h) = (frame.width(), frame.height());
            convert_rect(frame, IRect::frame(w, h), &mut buffer);
            buffer.present().map_err(|e| e.to_string())?;
            return Ok(true);
        }
        let mut rects = Vec::with_capacity(damage.len());
        for d in damage {
            convert_rect(frame, *d, &mut buffer);
            if let (Some(width), Some(height)) = (NonZeroU32::new(d.w), NonZeroU32::new(d.h)) {
                rects.push(Rect {
                    x: d.x as u32,
                    y: d.y as u32,
                    width,
                    height,
                });
            }
        }
        if exposed {
            buffer.present()
        } else {
            buffer.present_with_damage(&rects)
        }
        .map_err(|e| e.to_string())?;
        Ok(false)
    }
}
