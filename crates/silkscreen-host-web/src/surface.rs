//! A `<canvas>` as a [`Surface`]: an `ImageData` viewing the frame in wasm
//! memory, put a dirty rect at a time.

use silkscreen::IRect;
use silkscreen::core::tiny_skia::Pixmap;
use wasm_bindgen::{Clamped, JsValue};
use web_sys::{CanvasRenderingContext2d, ImageData};

use crate::memory::MemoryBinding;
use crate::present::Surface;

/// Puts frame rects onto a canvas's 2D context.
///
/// The `ImageData` is a view straight onto the frame's pixels, not a copy
/// ([design/rendering.md]). `ImageData` is unpremultiplied RGBA and the frame is
/// premultiplied; the two agree because faceplates are opaque, as on
/// native (`silkscreen-host-native` presents `0RGB`).
///
/// [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md
#[derive(Debug)]
pub struct CanvasSurface {
    ctx: CanvasRenderingContext2d,
    view: Option<(ImageData, MemoryBinding)>,
    binds: u32,
}

impl CanvasSurface {
    pub fn new(ctx: CanvasRenderingContext2d) -> Self {
        Self {
            ctx,
            view: None,
            binds: 0,
        }
    }

    /// How many times a view of the frame has been made. Grows on each
    /// resize and each wasm memory growth; for diagnostics.
    pub fn binds(&self) -> u32 {
        self.binds
    }
}

impl Surface for CanvasSurface {
    type Error = JsValue;

    fn is_bound(&self, frame: &Pixmap) -> bool {
        self.view
            .as_ref()
            .is_some_and(|(_, binding)| binding.sees(frame.data()))
    }

    fn bind(&mut self, frame: &Pixmap) -> Result<(), JsValue> {
        let data = frame.data();
        let image = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(data),
            frame.width(),
            frame.height(),
        )?;
        self.view = Some((image, MemoryBinding::of(data)));
        self.binds += 1;
        Ok(())
    }

    fn put(&mut self, rect: IRect) -> Result<(), JsValue> {
        let Some((image, _)) = &self.view else {
            return Ok(());
        };
        self.ctx
            .put_image_data_with_dirty_x_and_dirty_y_and_dirty_width_and_dirty_height(
                image,
                0.0,
                0.0,
                f64::from(rect.x),
                f64::from(rect.y),
                f64::from(rect.w),
                f64::from(rect.h),
            )
    }
}
