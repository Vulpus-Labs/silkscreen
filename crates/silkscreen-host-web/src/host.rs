//! The editor on a canvas: a `requestAnimationFrame` loop that also follows
//! the device pixel ratio, and the canvas's input.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use silkscreen::core::tiny_skia::Pixmap;
use silkscreen::core::{MouseCursor, RawKind, assets};
use silkscreen::{Editor, RawEvent};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::input::{self, Listener};
use crate::present::Presenter;
use crate::surface::CanvasSurface;
use crate::translate::{Translator, is_mac};

type FrameCallback = Closure<dyn FnMut(f64)>;

/// A running editor on a canvas. The loop and input stop on
/// [`stop`](Self::stop) or when this is dropped.
#[wasm_bindgen]
pub struct WebHost {
    inner: Rc<RefCell<Inner>>,
    raf: Rc<RefCell<Option<FrameCallback>>>,
    listeners: RefCell<Vec<Listener>>,
}

impl std::fmt::Debug for WebHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebHost").finish_non_exhaustive()
    }
}

pub(crate) struct Inner {
    pub(crate) editor: Editor,
    pub(crate) translator: Translator,
    canvas: HtmlCanvasElement,
    /// What the canvas's style shows.
    cursor: MouseCursor,
    presenter: Presenter<CanvasSurface>,
    frame: Pixmap,
    /// The device pixel ratio and zoom the frame was last sized for.
    dpr: f64,
    applied_zoom: f64,
    /// CSS pixels per logical pixel, from [`WebHost::set_zoom`]: the page
    /// fits the faceplate to its space with it.
    zoom: f64,
    /// Set when a fetched PNG arrives; the next frame repaints in full.
    assets_arrived: Rc<Cell<bool>>,
    /// Rects put since mount, for diagnostics.
    puts: u32,
}

/// CSS's name for `cursor`.
fn css_cursor(cursor: MouseCursor) -> &'static str {
    match cursor {
        MouseCursor::Hand => "pointer",
        MouseCursor::Grabbing => "grabbing",
        MouseCursor::Vertical => "ns-resize",
        MouseCursor::Horizontal => "ew-resize",
        MouseCursor::Text => "text",
        _ => "default",
    }
}

/// Show `editor` on `canvas` and start its loop. The canvas's CSS size is
/// set to the faceplate's logical size; its backing store follows the
/// device pixel ratio. The canvas becomes focusable (`tabindex` 0) and
/// takes pointer, wheel, key and file-drop input.
pub fn mount(canvas: HtmlCanvasElement, editor: Editor) -> Result<WebHost, JsValue> {
    let ctx = canvas
        .get_context("2d")?
        .ok_or("silkscreen: the canvas has no 2d context")?
        .dyn_into::<CanvasRenderingContext2d>()?;
    let navigator = window()?.navigator();
    let mac = is_mac(
        &navigator.platform().unwrap_or_default(),
        &navigator.user_agent().unwrap_or_default(),
    );
    let size = editor.state().layout().size;
    let mut inner = Inner {
        editor,
        translator: Translator::new(size, mac),
        canvas,
        cursor: MouseCursor::Default,
        presenter: Presenter::new(CanvasSurface::new(ctx)),
        frame: Pixmap::new(1, 1).expect("1x1"),
        dpr: 0.0,
        applied_zoom: 0.0,
        zoom: 1.0,
        assets_arrived: Rc::new(Cell::new(false)),
        puts: 0,
    };
    inner.apply_scale()?;
    // The first frame now, so the canvas is never seen empty.
    inner.draw()?;
    let canvas = inner.canvas.clone();
    let inner = Rc::new(RefCell::new(inner));
    let host = WebHost {
        listeners: RefCell::new(input::install(&inner, &canvas)?),
        inner,
        raf: Rc::new(RefCell::new(None)),
    };
    host.start()?;
    Ok(host)
}

#[wasm_bindgen]
impl WebHost {
    /// Stop the loop and remove the input listeners. The canvas keeps its
    /// last frame.
    pub fn stop(&self) {
        // Dropping the callback breaks its reference to its own slot; it
        // is never running here, as the loop and this call share a thread.
        self.raf.borrow_mut().take();
        self.listeners.borrow_mut().clear();
    }

    /// Rects put on the canvas since mount. Idle frames add none.
    pub fn puts(&self) -> u32 {
        self.inner.borrow().puts
    }

    /// Panels open in the editor (menus, overlays). While one is, keys
    /// are the editor's, so a page's own key bindings should stand aside.
    pub fn panels(&self) -> usize {
        self.inner.borrow().editor.panels()
    }

    /// Views made of the frame since mount: one per resize or growth of
    /// wasm memory.
    pub fn binds(&self) -> u32 {
        self.inner.borrow().presenter.surface().binds()
    }

    /// Show the faceplate at `zoom` CSS pixels per logical pixel, from the
    /// next frame. The editor renders at `devicePixelRatio × zoom`, so a
    /// zoomed faceplate stays sharp; input maps through the canvas's box at
    /// any zoom. Clamped to 0.25–4.
    #[wasm_bindgen(js_name = setZoom)]
    pub fn set_zoom(&self, zoom: f64) {
        if zoom.is_finite() {
            self.inner.borrow_mut().zoom = zoom.clamp(0.25, 4.0);
        }
    }

    /// The zoom last asked for.
    pub fn zoom(&self) -> f64 {
        self.inner.borrow().zoom
    }
}

impl WebHost {
    /// Run `f` on the editor, between frames.
    pub fn with_editor<R>(&self, f: impl FnOnce(&mut Editor) -> R) -> R {
        f(&mut self.inner.borrow_mut().editor)
    }

    fn start(&self) -> Result<(), JsValue> {
        let inner = Rc::clone(&self.inner);
        let slot = Rc::clone(&self.raf);
        let callback = Closure::<dyn FnMut(f64)>::new(move |now: f64| {
            if let Err(e) = inner.borrow_mut().step(now) {
                web_sys::console::error_1(&e);
            }
            if let Some(cb) = slot.borrow().as_ref() {
                let _ = request_frame(cb);
            }
        });
        request_frame(&callback)?;
        *self.raf.borrow_mut() = Some(callback);
        Ok(())
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Inner {
    /// Hand one input event to the editor, stamped with `performance.now()`.
    /// It is drawn on the next frame.
    pub(crate) fn input(&mut self, kind: RawKind) {
        self.editor.handle(&RawEvent::new(now_ms(), kind));
        let cursor = self.editor.cursor();
        if cursor != self.cursor {
            self.cursor = cursor;
            // The canvas only misses a cursor if its style is unreachable.
            let _ = self
                .canvas
                .style()
                .set_property("cursor", css_cursor(cursor));
        }
    }

    /// One animation frame. `now` is the frame's `performance.now()` time.
    fn step(&mut self, now: f64) -> Result<(), JsValue> {
        self.apply_scale()?;
        if self.assets_arrived.replace(false) {
            // Stand-ins are stale: repaint everything from the new PNGs.
            self.editor.set_scale((self.dpr * self.zoom) as f32);
        }
        self.editor.tick(now);
        self.draw()?;
        for url in assets::take_wanted() {
            fetch_asset(url, Rc::clone(&self.assets_arrived));
        }
        Ok(())
    }

    fn draw(&mut self) -> Result<(), JsValue> {
        let damage: &[_] = if self.editor.needs_render() {
            self.editor.render(&mut self.frame.as_mut())
        } else {
            &[]
        };
        self.puts += self.presenter.present(&self.frame, damage)? as u32;
        Ok(())
    }

    /// Follow the device pixel ratio, which browser zoom and moving between
    /// displays change, and the page's zoom: rescale the editor and resize
    /// the frame and canvas.
    ///
    /// The ratio is read every frame rather than watched with a `matchMedia`
    /// listener: reading it is one property access, and it catches every
    /// change, including emulated ones, which fire no media-query events.
    fn apply_scale(&mut self) -> Result<(), JsValue> {
        let dpr = window()?.device_pixel_ratio();
        if dpr != self.dpr || self.zoom != self.applied_zoom {
            self.dpr = dpr;
            self.applied_zoom = self.zoom;
            self.editor.set_scale((dpr * self.zoom) as f32);
            let (w, h) = self.editor.physical_size();
            self.frame = Pixmap::new(w, h).ok_or("silkscreen: empty editor")?;
            // Resizing the backing store clears it.
            self.canvas.set_width(w);
            self.canvas.set_height(h);
            let style = self.canvas.style();
            style.set_property("width", &format!("{}px", f64::from(w) / dpr))?;
            style.set_property("height", &format!("{}px", f64::from(h) / dpr))?;
            self.presenter.invalidate();
        }
        Ok(())
    }
}

fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| "silkscreen: no window".into())
}

/// Fetch `url` (relative to the page) into the asset store, and flag its
/// arrival. A failure is logged and not retried.
fn fetch_asset(url: &'static str, arrived: Rc<Cell<bool>>) {
    wasm_bindgen_futures::spawn_local(async move {
        let fetched = async {
            let response: web_sys::Response = JsFuture::from(window()?.fetch_with_str(url))
                .await?
                .dyn_into()?;
            if !response.ok() {
                return Err(JsValue::from(format!("HTTP {}", response.status())));
            }
            let buffer = JsFuture::from(response.array_buffer()?).await?;
            Ok::<_, JsValue>(js_sys::Uint8Array::new(&buffer).to_vec())
        };
        match fetched.await {
            Ok(bytes) => {
                assets::provide(url, bytes);
                arrived.set(true);
            }
            Err(e) => web_sys::console::error_2(&format!("silkscreen: fetching {url}:").into(), &e),
        }
    });
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn request_frame(callback: &FrameCallback) -> Result<i32, JsValue> {
    window()?.request_animation_frame(callback.as_ref().unchecked_ref())
}
