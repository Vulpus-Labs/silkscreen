//! Widgets that see the editor ([design/faceplates.md]): a plugin's canvas
//! control, and the built-in scope, meters and preset field.
//!
//! A core [`Widget`] draws from its control's [`WidgetState`] alone, and
//! repaints when a param its binding names changes. An [`EditorWidget`]
//! also catches up with the editor each time the view syncs (live
//! telemetry, the preset's name, several params at once) and says when
//! that needs a repaint. That is the one way display-only content
//! damages itself.
//!
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use silkscreen_core::telemetry::{Meter, MeterReading, MeterStyle, Scope, ScopeStyle, ScopeTrace};
use silkscreen_core::tiny_skia::{PixmapMut, Transform};
use silkscreen_core::{ControlId, Rect, Widget, WidgetState};

use crate::EditorState;

/// A control's widget that reads the editor. Give one to a control with
/// [`EditorState::set_widget`].
pub trait EditorWidget {
    /// Catch up with `state`: copy what [`draw`](Self::draw) needs, and
    /// return true if the control should repaint. Called whenever the
    /// view syncs, so keep it cheap; widgets that show only their
    /// binding's display needn't override it.
    fn update(&mut self, state: &EditorState) -> bool {
        let _ = state;
        false
    }

    /// Paint the control occupying logical `rect`, as
    /// [`Widget::draw`] does. `state` is the binding's display, with the
    /// view's hover, press and focus.
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    );
}

/// An editor widget shared between the view (updates) and the
/// compositor (drawing).
pub(crate) type Shared = Rc<RefCell<Box<dyn EditorWidget>>>;

/// Draws a shared editor widget as a core widget.
pub(crate) struct Drawn(pub Shared);

impl Widget for Drawn {
    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        self.0.borrow().draw(canvas, rect, transform, state);
    }
}

/// The scope control (design/engine-bridge.md): the editor's latest trace.
pub(crate) struct LiveScope {
    scope: Scope,
    trace: ScopeTrace,
    /// The trace generation last copied.
    seen: Option<u64>,
}

impl LiveScope {
    pub fn new(style: ScopeStyle) -> Self {
        let trace = ScopeTrace::default();
        Self {
            scope: Scope::new(style, Rc::clone(&trace)),
            trace,
            seen: None,
        }
    }
}

impl EditorWidget for LiveScope {
    fn update(&mut self, state: &EditorState) -> bool {
        let (trace, generation) = state.scope_trace();
        if self.seen == Some(generation) {
            return false;
        }
        self.seen = Some(generation);
        let mut copy = self.trace.borrow_mut();
        copy.clear();
        copy.extend_from_slice(trace);
        true
    }

    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        self.scope.draw(canvas, rect, transform, state);
    }
}

/// A meter control (design/engine-bridge.md): its source's latest reading.
pub(crate) struct LiveMeter {
    control: ControlId,
    meter: Meter,
    reading: Rc<Cell<MeterReading>>,
    seen: Option<u64>,
}

impl LiveMeter {
    pub fn new(control: ControlId, style: MeterStyle) -> Self {
        let reading = Rc::default();
        Self {
            control,
            meter: Meter::new(style, Rc::clone(&reading)),
            reading,
            seen: None,
        }
    }
}

impl EditorWidget for LiveMeter {
    fn update(&mut self, state: &EditorState) -> bool {
        let Some((reading, generation)) = state.meter_reading(self.control) else {
            return false;
        };
        if self.seen == Some(generation) {
            return false;
        }
        self.seen = Some(generation);
        self.reading.set(reading);
        true
    }

    fn draw(
        &self,
        canvas: &mut PixmapMut<'_>,
        rect: Rect,
        transform: Transform,
        state: &WidgetState,
    ) {
        self.meter.draw(canvas, rect, transform, state);
    }
}
