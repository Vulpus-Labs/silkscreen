//! The baseview window handler that hosts an editor view.
//!
//! The long-lived [`EditorState`] is shared as `Rc<RefCell<_>>`: a plugin
//! keeps it for its whole life and lends it to each window it opens; a
//! standalone app makes one for its single window. The window owns the
//! per-window [`EditorView`]. Everything here runs on the main thread.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use baseview::dpi::LogicalSize;
use baseview::{
    DamageArea, DropEffect, Event, EventStatus, HandlerError, MouseEvent, TimerHandle, Window,
    WindowContext, WindowHandler, WindowSettings, WindowSize,
};
use keyboard_types::{Key as KtKey, KeyState, KeyboardEvent};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use silkscreen::core::tiny_skia::Pixmap;
use silkscreen::core::{MouseCursor, RasterSource, Size};
use silkscreen::{Editor, EditorState, EditorView, RawEvent};

use crate::menu::MenuCommand;
use crate::present::Presenter;
use crate::preset_io::NativePresetIo;
use crate::translate::Translator;
use crate::zoom::{ZOOM_STEPS, zoom_for_size};

/// Interval of the tick timer.
const TICK: Duration = Duration::from_millis(16);

/// baseview's cursor for `cursor`.
fn mouse_cursor(cursor: MouseCursor) -> baseview::MouseCursor {
    match cursor {
        MouseCursor::Hand => baseview::MouseCursor::Hand,
        MouseCursor::Grabbing => baseview::MouseCursor::HandGrabbing,
        MouseCursor::Vertical => baseview::MouseCursor::NsResize,
        MouseCursor::Horizontal => baseview::MouseCursor::EwResize,
        MouseCursor::Text => baseview::MouseCursor::Text,
        _ => baseview::MouseCursor::Default,
    }
}

/// A shared handle to an editor's long-lived state.
pub type SharedState = Rc<RefCell<EditorState>>;

/// Window options.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    /// Logical size of the faceplate (generated `SIZE`).
    pub size: Size,
    /// Initial zoom; one of [`ZOOM_STEPS`].
    pub zoom: f32,
    /// Number keys 1–5 select [`ZOOM_STEPS`]. For previews; plugins resize
    /// through the host instead.
    pub zoom_keys: bool,
    /// The window is embedded in a DAW, which has keyboard shortcuts of
    /// its own (design/input.md). Keys go to the editor only while a panel
    /// takes typed text (the preset browser's search), and otherwise on
    /// to the DAW; names are typed in a prompt window of the platform's
    /// (macOS, Windows), since the DAW takes some keys (Space) first.
    pub in_host: bool,
}

impl WindowOptions {
    pub fn new(title: impl Into<String>, size: Size) -> Self {
        Self {
            title: title.into(),
            size,
            zoom: 1.0,
            zoom_keys: true,
            in_host: false,
        }
    }
}

/// Open `make()`'s editor in a top-level window and run until it closes.
/// `make` runs on the window's thread.
pub fn run_standalone(
    options: WindowOptions,
    make: impl FnOnce() -> Editor + Send + 'static,
) -> Result<(), baseview::Error> {
    // Resizable between the smallest and largest zoom steps: baseview
    // ignores `WindowContext::resize` (the zoom keys) on a fixed-size
    // window. A user's drag snaps to a step too (see `resized`).
    let settings = WindowSettings::new()
        .with_title(options.title.clone())
        .with_size(logical_size(options.size, options.zoom))
        .with_resizable(true)
        .with_min_size(logical_size(options.size, ZOOM_STEPS[0]))
        .with_max_size(logical_size(options.size, ZOOM_STEPS[ZOOM_STEPS.len() - 1]));
    let window = Window::create(settings, move |ctx| {
        // The application exists by now, on this (the main) thread.
        crate::menu::install(&options.title);
        let Editor { state, view } = make();
        let mut window = EditorWindow::new(ctx, Rc::new(RefCell::new(state)), view, &options)?;
        window.snap_to_zoom = true;
        Ok(window)
    })?;
    window.run_until_closed()
}

/// A host window an editor can be embedded in.
#[derive(Debug, Clone, Copy)]
pub struct ParentWindow(RawWindowHandle);

impl ParentWindow {
    /// # Safety
    ///
    /// `raw` must be a valid window handle that stays alive for as long as
    /// any [`EmbeddedWindow`] opened in it. Plugin hosts guarantee this for
    /// the parent they hand over until the editor is destroyed.
    #[allow(unsafe_code)]
    pub unsafe fn new(raw: RawWindowHandle) -> Self {
        Self(raw)
    }
}

impl HasWindowHandle for ParentWindow {
    #[allow(unsafe_code)]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: validity for the window's lifetime is the contract of
        // `ParentWindow::new`.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

/// An editor embedded in a host window. Closing it (or dropping it) ends
/// any drag in progress, so no host gesture is left open.
pub struct EmbeddedWindow {
    window: Option<Window>,
    ticker: Option<Ticker>,
}

/// Lets the embedding host drive the editor's tick from its own timer.
struct Ticker {
    ctx: WindowContext,
    inner: Weak<RefCell<Inner>>,
}

impl std::fmt::Debug for EmbeddedWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedWindow").finish_non_exhaustive()
    }
}

impl EmbeddedWindow {
    pub fn show(&self) {
        if let Some(w) = &self.window {
            let _ = w.show();
        }
    }

    pub fn hide(&self) {
        if let Some(w) = &self.window {
            let _ = w.hide();
        }
    }

    /// Advance the editor: pull host changes, fire dwells, and request a
    /// redraw if anything changed. Plugins call this from the host's timer,
    /// which the host guarantees to run; baseview's own timer is scheduled
    /// in the default run-loop mode only, and a host may run its loop in
    /// another (seen in Reaper: host automation didn't repaint the knob).
    pub fn tick(&self) {
        let Some(t) = &self.ticker else { return };
        let Some(inner) = t.inner.upgrade() else {
            return;
        };
        let Ok(mut inner) = inner.try_borrow_mut() else {
            return;
        };
        inner.tick();
        if inner.repaint == Repaint::Full || inner.view.needs_render() {
            t.ctx.request_redraw();
        }
    }

    /// Change the zoom (one of [`ZOOM_STEPS`]): resizes the window and
    /// repaints it fully at the new effective scale. For hosts that resize
    /// the editor (CLAP `gui.set_size`).
    pub fn set_zoom(&self, zoom: f32) {
        let (Some(window), Some(t)) = (&self.window, &self.ticker) else {
            return;
        };
        let Some(inner) = t.inner.upgrade() else {
            return;
        };
        let size = {
            let Ok(mut inner) = inner.try_borrow_mut() else {
                return;
            };
            if inner.zoom == zoom {
                return;
            }
            inner.zoom = zoom;
            inner.size
        };
        // `resized` follows, possibly synchronously, and applies the scale;
        // no borrow may be held across this call.
        let _ = window.resize(logical_size(size, zoom));
        t.ctx.request_redraw();
    }

    /// Show (or clear) a diagnostic label in the editor's corner.
    pub fn set_badge(&self, text: Option<String>) {
        let Some(t) = &self.ticker else { return };
        let Some(inner) = t.inner.upgrade() else {
            return;
        };
        let Ok(mut inner) = inner.try_borrow_mut() else {
            return;
        };
        inner.view.set_badge(text);
        if inner.view.needs_render() {
            t.ctx.request_redraw();
        }
    }

    pub fn close(&mut self) {
        self.ticker = None;
        if let Some(w) = self.window.take() {
            w.close();
        }
    }
}

impl Drop for EmbeddedWindow {
    fn drop(&mut self) {
        self.close();
    }
}

/// The arguments [`open_embedded`] passes through baseview's builder.
type Handoff = (SharedState, Box<dyn RasterSource>);

thread_local! {
    /// Hands non-`Send` construction arguments to baseview's handler
    /// builder, which baseview requires to be `Send` but (on macOS) runs
    /// synchronously on this thread inside `Window::create`.
    static HANDOFF: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    /// What the builder hands back: the window's ticker.
    static TICKER: RefCell<Option<Ticker>> = const { RefCell::new(None) };
}

/// Open a view of `state` inside `parent`, drawing the faceplate from
/// `raster`. Must be called on the main thread.
pub fn open_embedded(
    parent: &ParentWindow,
    options: WindowOptions,
    state: SharedState,
    raster: Box<dyn RasterSource>,
) -> Result<EmbeddedWindow, baseview::Error> {
    let handoff: Handoff = (state, raster);
    HANDOFF.with(|h| *h.borrow_mut() = Some(Box::new(handoff)));
    let settings = WindowSettings::new()
        .with_title(options.title.clone())
        .with_size(logical_size(options.size, options.zoom))
        .with_resizable(false)
        .with_parent(parent);
    let result = Window::create(settings, move |ctx| {
        let handed = HANDOFF.with(|h| h.borrow_mut().take()).ok_or_else(|| {
            HandlerError::from_boxed(
                "silkscreen: the window builder ran on another thread; embedded editors are not supported on this platform yet"
                    .into(),
            )
        })?;
        let (state, raster) = *handed.downcast::<Handoff>().map_err(|_| {
            HandlerError::from_boxed("silkscreen: window handoff type mismatch".into())
        })?;
        let view = EditorView::new(&mut state.borrow_mut(), raster);
        let window = EditorWindow::new(ctx.clone(), state, view, &options)?;
        TICKER.with(|t| {
            *t.borrow_mut() = Some(Ticker {
                ctx,
                inner: Rc::downgrade(&window.inner),
            });
        });
        Ok(window)
    });
    // Never leave a stale handoff behind, whether or not the builder ran.
    HANDOFF.with(|h| h.borrow_mut().take());
    let ticker = TICKER.with(|t| t.borrow_mut().take());
    Ok(EmbeddedWindow {
        window: Some(result?),
        ticker,
    })
}

fn logical_size(size: Size, zoom: f32) -> LogicalSize<f64> {
    LogicalSize::new(f64::from(size.w * zoom), f64::from(size.h * zoom))
}

fn handler_error(e: impl std::error::Error + 'static) -> HandlerError {
    HandlerError::from_boxed(Box::new(e))
}

struct EditorWindow {
    ctx: WindowContext,
    inner: Rc<RefCell<Inner>>,
    /// Top-level windows the user can resize: a new size picks the nearest
    /// zoom step, and the window then settles to that step's exact size.
    /// Embedded windows are sized by their host instead.
    snap_to_zoom: bool,
    /// macOS calls the view back while the editor is busy: opening a
    /// prompt's sheet takes key from the DAW's window, which tells the
    /// view at once. Re-entered, a callback can't borrow `inner`,
    /// so a draw is marked missed and a resize kept, both for the next
    /// tick, and an event is dropped.
    missed_draw: Cell<bool>,
    pending_resize: Cell<Option<WindowSize>>,
}

/// The least time between two ticks of the view, ms: half the 16 ms
/// period both clocks run at.
const MIN_TICK_MS: f64 = 8.0;

/// How much of the frame the next present sends to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Repaint {
    /// Only what the render damaged.
    Damage,
    /// The OS lost what the window showed (`WM_PAINT`, X11 `Expose`): the
    /// whole buffer goes to the window, though only the damage is
    /// converted. On Windows the frame presented in `new`, before the
    /// window is shown, never reaches the screen; without this the
    /// faceplate stays blank until something over it redraws.
    Exposed,
    /// The whole frame, converted and sent (after a resize, or first).
    Full,
}

struct Inner {
    state: SharedState,
    view: EditorView,
    /// What the window shows.
    cursor: MouseCursor,
    presenter: Presenter,
    frame: Pixmap,
    translator: Translator,
    size: Size,
    zoom: f32,
    zoom_keys: bool,
    in_host: bool,
    /// When the view last ticked: a plugin window has two clocks (the
    /// window's timer and the host's), and one tick a period is enough.
    last_tick: Option<f64>,
    /// How much the next present must cover.
    repaint: Repaint,
    _timer: TimerHandle,
}

impl EditorWindow {
    fn new(
        ctx: WindowContext,
        state: SharedState,
        view: EditorView,
        options: &WindowOptions,
    ) -> Result<Self, HandlerError> {
        let presenter = Presenter::new(&ctx.platform_handle()).map_err(HandlerError::from_boxed)?;
        // Export and Import through dialogs over this window (design/presets.md).
        state
            .borrow_mut()
            .set_preset_io(Rc::new(NativePresetIo::new(ctx.clone())));
        // Paste, and Cmd/Ctrl-V in fields.
        if let Some(clipboard) = crate::clipboard::system() {
            state.borrow_mut().set_clipboard(clipboard);
        }
        // Names typed in a window of their own, in a DAW.
        if options.in_host
            && let Some(prompt) = crate::prompt::for_window(&ctx.platform_handle())
        {
            state.borrow_mut().set_text_prompt(prompt);
        }
        let timer = ctx.create_timer(TICK).map_err(handler_error)?;
        let display = ctx.scale_factor();
        let mut inner = Inner {
            state,
            view,
            cursor: MouseCursor::Default,
            presenter,
            frame: Pixmap::new(1, 1).expect("1x1"),
            translator: Translator::new(display, display * f64::from(options.zoom)),
            size: options.size,
            zoom: options.zoom,
            zoom_keys: options.zoom_keys,
            in_host: options.in_host,
            last_tick: None,
            repaint: Repaint::Full,
            _timer: timer,
        };
        inner.apply_scale(display)?;
        // Present the first frame now, before the view is shown. Otherwise
        // it appears empty until the next display pass, a visible flash
        // when a host swaps one plugin editor for another.
        inner.draw()?;
        Ok(Self {
            ctx,
            inner: Rc::new(RefCell::new(inner)),
            snap_to_zoom: false,
            missed_draw: Cell::new(false),
            pending_resize: Cell::new(None),
        })
    }

    /// A new window size: the zoom it snaps to (top-level windows) and
    /// the scale.
    fn resize_inner(&self, inner: &mut Inner, new_size: WindowSize) -> Result<(), HandlerError> {
        if self.snap_to_zoom {
            let size = new_size.logical;
            inner.zoom = zoom_for_size(inner.size, size.width, size.height);
        }
        inner.apply_scale(new_size.scale_factor)?;
        self.redraw_if_needed(inner);
        Ok(())
    }

    fn redraw_if_needed(&self, inner: &Inner) {
        if inner.repaint == Repaint::Full || inner.view.needs_render() {
            self.ctx.request_redraw();
        }
    }

    /// The zoom step a key selects, if it is a zoom key.
    fn zoom_key(inner: &Inner, event: &KeyboardEvent) -> Option<f32> {
        // With a panel open (a text field, a menu), keys belong to it.
        if !inner.zoom_keys || event.state != KeyState::Down || inner.view.panels() > 0 {
            return None;
        }
        let KtKey::Character(s) = &event.key else {
            return None;
        };
        let step = s.chars().next().and_then(|c| c.to_digit(10))?;
        ZOOM_STEPS.get((step as usize).wrapping_sub(1)).copied()
    }
}

/// Milliseconds on the process's editor clock: one origin for every
/// window, so time never runs backwards across a close and reopen. The
/// editor state outlives its window and keeps times from it (when the
/// scope last refreshed, a meter's hold); a window starting its own clock
/// at zero froze the scope and meters until it caught up with the last.
fn clock_ms() -> f64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

impl Inner {
    /// Tick the view, unless it ticked within half a period.
    fn tick(&mut self) {
        let now = clock_ms();
        if self.last_tick.is_some_and(|t| now - t < MIN_TICK_MS) {
            return;
        }
        self.last_tick = Some(now);
        self.with_state(|view, state| view.tick(state, now));
    }

    /// Run `f` with the view and the shared state. Skips (rather than
    /// panicking) if the state is already borrowed, which would only happen
    /// if a host re-entered the plugin from inside a window callback.
    fn with_state(&mut self, f: impl FnOnce(&mut EditorView, &mut EditorState)) {
        let state = self.state.clone();
        if let Ok(mut s) = state.try_borrow_mut() {
            f(&mut self.view, &mut s);
        }
    }

    /// Re-derive the effective scale and reallocate buffers.
    fn apply_scale(&mut self, display: f64) -> Result<(), HandlerError> {
        let effective = display * f64::from(self.zoom);
        self.translator = Translator::new(display, effective);
        self.view.set_scale(effective as f32);
        let (w, h) = self.view.physical_size();
        self.frame = Pixmap::new(w, h).expect("non-empty editor");
        self.presenter
            .resize(w, h, display)
            .map_err(HandlerError::from_boxed)?;
        self.repaint = Repaint::Full;
        Ok(())
    }

    fn draw(&mut self) -> Result<(), HandlerError> {
        let mut damage = Vec::new();
        let state = self.state.clone();
        if let Ok(mut s) = state.try_borrow_mut() {
            damage = self.view.render(&mut s, &mut self.frame.as_mut()).to_vec();
        }
        if self
            .presenter
            .present(
                &self.frame,
                &damage,
                self.repaint == Repaint::Full,
                self.repaint == Repaint::Exposed,
            )
            .map_err(HandlerError::from_boxed)?
            || self.repaint == Repaint::Exposed
        {
            self.repaint = Repaint::Damage;
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The window is going away: close any drag so its gesture ends.
        self.with_state(EditorView::cancel);
    }
}

impl WindowHandler for EditorWindow {
    fn draw(&self) -> Result<(), HandlerError> {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            self.missed_draw.set(true);
            return Ok(());
        };
        inner.draw()
    }

    fn damage(&self, _area: DamageArea) {
        // Windows reports its whole update region with every redraw, ours
        // included, so the area is no guide: send the whole buffer. Only
        // the damage is converted, so this costs a blit, not a render.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.repaint = inner.repaint.max(Repaint::Exposed);
        }
    }

    fn resized(&self, new_size: WindowSize) -> Result<(), HandlerError> {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            self.pending_resize.set(Some(new_size));
            return Ok(());
        };
        self.resize_inner(&mut inner, new_size)
    }

    fn on_event(&self, event: Event) -> EventStatus {
        // Re-entered (see `missed_draw`): the editor is mid-call, and
        // what reaches it this way is a focus or pointer change it can
        // do without.
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return EventStatus::Ignored;
        };
        if let Event::Keyboard(k) = &event
            && let Some(zoom) = Self::zoom_key(&inner, k)
        {
            if zoom != inner.zoom {
                inner.zoom = zoom;
                let size = logical_size(inner.size, zoom);
                drop(inner);
                // `resized` follows, possibly synchronously (macOS), and
                // applies the new scale; it needs the borrow released.
                let _ = self.ctx.resize(size);
            }
            return EventStatus::Captured;
        }
        // A key the editor doesn't want is the host's: `Ignored` passes it
        // up the responder chain (macOS) to the DAW.
        if matches!(event, Event::Keyboard(_)) && inner.in_host && !inner.view.takes_text() {
            return EventStatus::Ignored;
        }
        let accept_drop = matches!(
            event,
            Event::Mouse(MouseEvent::DragEntered { .. } | MouseEvent::DragMoved { .. })
        );
        if let Some(kind) = inner.translator.translate(&event) {
            let raw = RawEvent::new(clock_ms(), kind);
            let mut cursor = None;
            inner.with_state(|view, state| {
                view.handle(state, &raw);
                cursor = Some(view.cursor(state));
            });
            if let Some(cursor) = cursor
                && cursor != inner.cursor
            {
                inner.cursor = cursor;
                let _ = self.ctx.set_mouse_cursor(mouse_cursor(cursor));
            }
        }
        self.redraw_if_needed(&inner);
        if accept_drop {
            EventStatus::AcceptDrop(DropEffect::Copy)
        } else {
            EventStatus::Captured
        }
    }

    fn on_timer(&self, _timer: &TimerHandle) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        // What re-entered callbacks left for now (see `missed_draw`).
        if let Some(size) = self.pending_resize.take() {
            let _ = self.resize_inner(&mut inner, size);
        }
        if self.missed_draw.take() {
            inner.repaint = Repaint::Full;
        }
        // The menu bar's Undo and Redo (a standalone window's only).
        for command in crate::menu::take() {
            inner.with_state(|_, state| {
                match command {
                    MenuCommand::Undo => state.undo(),
                    MenuCommand::Redo => state.redo(),
                };
            });
        }
        inner.tick();
        self.redraw_if_needed(&inner);
        if self.snap_to_zoom {
            let want = logical_size(inner.size, inner.zoom);
            let have = self.ctx.size().logical;
            if (have.width - want.width).abs() > 0.5 || (have.height - want.height).abs() > 0.5 {
                drop(inner);
                // `resized` may follow synchronously; no borrow is held.
                let _ = self.ctx.resize(want);
            }
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::clock_ms;

    #[test]
    fn the_clock_is_shared_and_never_restarts() {
        let before = clock_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        // A window opened now reads the same clock, already past `before`.
        assert!(clock_ms() >= before + 5.0, "the clock restarted");
    }
}
