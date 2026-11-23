//! macOS: present into a sublayer of the view's layer, whose `contents` is a
//! `CGImage` of the frame. This is what softbuffer's Core Graphics backend
//! does, minus its key-value observer: that is a fixed-name Objective-C
//! class, and a second plugin registering it in the same process panics
//!. We register no class; the window code tells us the size
//! and scale instead.
#![allow(unsafe_code)] // Objective-C and Core Graphics FFI

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityTopLeft};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use silkscreen::IRect;
use silkscreen::core::tiny_skia::Pixmap;

use super::PresentError;

pub(crate) struct Presenter {
    /// Our sublayer of the view's layer.
    layer: Retained<CALayer>,
    root: Retained<CALayer>,
    color_space: CFRetained<CGColorSpace>,
}

impl Presenter {
    pub fn new(window: &impl HasWindowHandle) -> Result<Self, PresentError> {
        MainThreadMarker::new().ok_or("the presenter must be created on the main thread")?;
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("not an AppKit window".into());
        };
        // SAFETY: `WindowHandle` guarantees a valid `NSView` pointer, and we
        // are on the main thread (checked above).
        let view: &NSObject = unsafe { handle.ns_view.cast().as_ref() };
        let _: () = unsafe { msg_send![view, setWantsLayer: Bool::YES] };
        // SAFETY: `-[NSView layer]` returns an optional `CALayer`.
        let root: Option<Retained<CALayer>> = unsafe { msg_send![view, layer] };
        let root = root.ok_or("the view has no layer")?;

        // A sublayer, so we never fight AppKit over the view's own layer.
        let layer = CALayer::new();
        root.addSublayer(&layer);
        // Top-left origin, like the frame buffer.
        layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        layer.setGeometryFlipped(true);
        // SAFETY: a constant `NSString` exported by QuartzCore.
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });
        let color_space =
            CGColorSpace::new_device_rgb().ok_or("could not create an RGB color space")?;
        Ok(Self {
            layer,
            root,
            color_space,
        })
    }

    /// Match the view's bounds, at `display_scale` pixels per point. Same
    /// signature as the softbuffer presenter's, which can fail.
    #[allow(clippy::unnecessary_wraps)]
    pub fn resize(&mut self, _w: u32, _h: u32, display_scale: f64) -> Result<(), PresentError> {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layer.setFrame(self.root.bounds());
        self.layer.setContentsScale(display_scale);
        CATransaction::commit();
        Ok(())
    }

    /// Show `frame`. Core Animation takes a whole image and keeps it, so
    /// damage and exposure only matter to the caller; `true` means the
    /// whole frame was presented.
    pub fn present(
        &mut self,
        frame: &Pixmap,
        _damage: &[IRect],
        _full: bool,
        _exposed: bool,
    ) -> Result<bool, PresentError> {
        unsafe extern "C-unwind" fn release(
            _info: *mut c_void,
            data: NonNull<c_void>,
            size: usize,
        ) {
            let slice = ptr::slice_from_raw_parts_mut(data.as_ptr().cast::<u8>(), size);
            // SAFETY: the `Box<[u8]>` leaked below, handed back exactly once.
            drop(unsafe { Box::from_raw(slice) });
        }

        let (w, h) = (frame.width() as usize, frame.height() as usize);
        // Core Graphics may read the pixels after this returns, so it gets
        // its own copy, freed by `release`.
        let bytes: Box<[u8]> = frame.data().into();
        let len = bytes.len();
        let data = Box::into_raw(bytes).cast::<c_void>();
        // SAFETY: `data` is valid for `len` bytes until `release` frees it.
        let provider =
            unsafe { CGDataProvider::with_data(ptr::null_mut(), data, len, Some(release)) }
                .ok_or("could not create a data provider")?;

        // The frame is opaque RGBA8 in memory order: R, G, B, then an
        // ignored alpha byte. No swizzle needed.
        let info = CGBitmapInfo(
            CGImageAlphaInfo::NoneSkipLast.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::OrderDefault.0
                | CGImagePixelFormatInfo::Packed.0,
        );
        // SAFETY: the dimensions and stride describe `provider`'s bytes.
        let image = unsafe {
            CGImage::new(
                w,
                h,
                8,
                32,
                w * 4,
                Some(&self.color_space),
                info,
                Some(&provider),
                ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .ok_or("could not create an image")?;

        // Without this, every new image fades in over a quarter second.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: a `CGImage` is a valid `contents` value.
        unsafe { self.layer.setContents(Some(image.as_ref())) };
        CATransaction::commit();
        Ok(true)
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        self.layer.removeFromSuperlayer();
    }
}
