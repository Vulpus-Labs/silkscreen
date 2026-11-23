//! DOM listeners on the canvas: read each event's fields, translate them
//! ([`crate::translate`]) and hand the result to the editor.

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen::core::{ControlKind, DropPayload, RawKind};
use wasm_bindgen::prelude::*;
use web_sys::{
    AddEventListenerOptions, DragEvent, Event, EventTarget, HtmlCanvasElement, KeyboardEvent,
    MouseEvent, PointerEvent, TouchEvent, WheelEvent,
};

use crate::files;
use crate::host::Inner;
use crate::translate::{CanvasBox, DomModifiers, DomPointer, suppress_key_default};

/// An installed event listener, removed when dropped.
pub(crate) struct Listener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}

fn listen(
    target: &EventTarget,
    name: &'static str,
    f: impl FnMut(Event) + 'static,
) -> Result<Listener, JsValue> {
    let callback = Closure::<dyn FnMut(Event)>::new(f);
    // Not passive: the handlers suppress defaults (wheel scrolling over a
    // control above all), which a passive listener can't.
    let options = AddEventListenerOptions::new();
    options.set_passive(false);
    target.add_event_listener_with_callback_and_add_event_listener_options(
        name,
        callback.as_ref().unchecked_ref(),
        &options,
    )?;
    Ok(Listener {
        target: target.clone(),
        name,
        callback,
    })
}

/// Make `canvas` focusable, and route its events to `inner`'s editor.
///
/// A touch may pan or pinch the page, so a faceplate wider than a phone can
/// be scrolled, except where the editor wants it: a touch that lands on a
/// control, or anywhere while a panel is open, is the editor's
/// ([`touch_start`]).
pub(crate) fn install(
    inner: &Rc<RefCell<Inner>>,
    canvas: &HtmlCanvasElement,
) -> Result<Vec<Listener>, JsValue> {
    canvas.set_tab_index(0);
    let style = canvas.style();
    style.set_property("touch-action", "manipulation")?;
    style.set_property("user-select", "none")?;
    style.set_property("-webkit-user-select", "none")?;
    style.set_property("outline", "none")?;

    let target: &EventTarget = canvas.as_ref();
    let mut listeners = Vec::new();

    let on = |f: fn(&mut Inner, &HtmlCanvasElement, &Event)| {
        let inner = Rc::downgrade(inner);
        let canvas = canvas.clone();
        move |event: Event| {
            if let Some(inner) = inner.upgrade()
                && let Ok(mut inner) = inner.try_borrow_mut()
            {
                f(&mut inner, &canvas, &event);
            }
        }
    };

    listeners.push(listen(target, "pointerdown", on(pointer_down))?);
    listeners.push(listen(target, "pointermove", on(pointer_move))?);
    listeners.push(listen(target, "pointerup", on(pointer_up))?);
    listeners.push(listen(target, "pointercancel", on(pointer_up))?);
    // However capture ends, the press ends with it. After a `pointerup`
    // this finds nothing held and sends nothing.
    listeners.push(listen(target, "lostpointercapture", on(pointer_up))?);
    listeners.push(listen(target, "pointerleave", on(pointer_leave))?);
    listeners.push(listen(target, "touchstart", on(touch_start))?);
    listeners.push(listen(target, "wheel", on(wheel))?);
    listeners.push(listen(target, "keydown", on(key))?);
    listeners.push(listen(target, "keyup", on(key))?);
    listeners.push(listen(target, "blur", on(blur))?);
    listeners.push(listen(target, "dragover", on(drag_over))?);
    listeners.push(listen(target, "drop", on(file_drop))?);
    // Right-click belongs to the editor. Native `dblclick` is never
    // listened to: double-clicks come from the recognizer.
    listeners.push(listen(target, "contextmenu", |e: Event| {
        e.prevent_default();
    })?);
    Ok(listeners)
}

fn canvas_box(canvas: &HtmlCanvasElement) -> CanvasBox {
    let r = canvas.get_bounding_client_rect();
    CanvasBox {
        left: r.left(),
        top: r.top(),
        width: r.width(),
        height: r.height(),
    }
}

fn dom_pointer(e: &MouseEvent) -> DomPointer {
    DomPointer {
        client_x: f64::from(e.client_x()),
        client_y: f64::from(e.client_y()),
        button: e.button(),
        buttons: e.buttons(),
        mods: DomModifiers {
            shift: e.shift_key(),
            alt: e.alt_key(),
            ctrl: e.ctrl_key(),
            meta: e.meta_key(),
        },
    }
}

/// The primary pointer's event, if `event` is one. Extra touch points
/// and pens beside the mouse are ignored.
fn primary(event: &Event) -> Option<&PointerEvent> {
    event.dyn_ref::<PointerEvent>().filter(|p| p.is_primary())
}

fn pointer_down(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    let Some(p) = primary(event) else { return };
    // Captured, the drag keeps reporting outside the canvas and the window
    // until release. Suppressing the default stops text selection, and with
    // it the focus that would come with it, so focus explicitly.
    let _ = canvas.set_pointer_capture(p.pointer_id());
    event.prevent_default();
    let _ = canvas.focus();
    for kind in inner
        .translator
        .pointer_down(dom_pointer(p), canvas_box(canvas))
    {
        inner.input(kind);
    }
}

/// Whether a touch or wheel at client point (`x`, `y`) is the editor's: on
/// a control that takes input (on the shown page), or anywhere while a
/// panel is open. Display-only controls (the scope, meters) take neither.
fn editors_at(inner: &Inner, canvas: &HtmlCanvasElement, x: f64, y: f64) -> bool {
    if inner.editor.panels() > 0 {
        return true;
    }
    let state = inner.editor.state();
    let p = inner.translator.logical(x, y, canvas_box(canvas));
    state
        .layout()
        .hit_test_shown(p, state.shown_pages())
        .is_some_and(|id| {
            !matches!(
                state.layout().control(id).kind,
                ControlKind::Scope | ControlKind::Meter
            )
        })
}

/// A touch that is the editor's ([`editors_at`]) has its default
/// suppressed, so the page doesn't scroll and the drag turns the control.
/// Elsewhere the page pans and pinches as usual; the browser then cancels
/// the pointer, which reaches the editor as a release.
fn touch_start(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    let Some(touches) = event
        .dyn_ref::<TouchEvent>()
        .map(TouchEvent::changed_touches)
    else {
        return;
    };
    let mut ours = false;
    for i in 0..touches.length() {
        if let Some(t) = touches.item(i) {
            ours |= editors_at(
                inner,
                canvas,
                f64::from(t.client_x()),
                f64::from(t.client_y()),
            );
        }
    }
    if ours {
        event.prevent_default();
    }
}

fn pointer_move(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    let Some(p) = primary(event) else { return };
    for kind in inner
        .translator
        .pointer_move(dom_pointer(p), canvas_box(canvas))
    {
        inner.input(kind);
    }
}

fn pointer_up(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    let Some(p) = primary(event) else { return };
    if let Some(kind) = inner
        .translator
        .pointer_up(dom_pointer(p), canvas_box(canvas))
    {
        inner.input(kind);
    }
}

fn pointer_leave(inner: &mut Inner, _: &HtmlCanvasElement, event: &Event) {
    if primary(event).is_some() {
        inner.input(RawKind::PointerLeave);
    }
}

fn wheel(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    let Some(w) = event.dyn_ref::<WheelEvent>() else {
        return;
    };
    // The wheel scrolls the page except where it's the editor's, as a touch
    // pans it. A wheel the browser has already started scrolling with
    // comes uncancelable for the rest of the gesture (Chrome's scroll
    // latching): a knob that scrolls under the pointer doesn't turn then.
    if !w.cancelable()
        || !editors_at(
            inner,
            canvas,
            f64::from(w.client_x()),
            f64::from(w.client_y()),
        )
    {
        return;
    }
    event.prevent_default();
    let kind = inner.translator.wheel(
        dom_pointer(w),
        w.delta_x(),
        w.delta_y(),
        w.delta_mode(),
        canvas_box(canvas),
    );
    inner.input(kind);
}

fn key(inner: &mut Inner, _: &HtmlCanvasElement, event: &Event) {
    let Some(k) = event.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    let name = k.key();
    if suppress_key_default(&name, inner.editor.panels() > 0) {
        event.prevent_default();
    }
    let mods = DomModifiers {
        shift: k.shift_key(),
        alt: k.alt_key(),
        ctrl: k.ctrl_key(),
        meta: k.meta_key(),
    };
    if let Some(kind) = inner
        .translator
        .key(&name, mods, event.type_() == "keydown")
    {
        inner.input(kind);
    }
}

fn blur(inner: &mut Inner, _: &HtmlCanvasElement, _: &Event) {
    inner.input(RawKind::FocusLost);
}

fn drag_over(_: &mut Inner, _: &HtmlCanvasElement, event: &Event) {
    // Accepting the drag is what makes `drop` fire.
    event.prevent_default();
    if let Some(dt) = event
        .dyn_ref::<DragEvent>()
        .and_then(DragEvent::data_transfer)
    {
        dt.set_drop_effect("copy");
    }
}

fn file_drop(inner: &mut Inner, canvas: &HtmlCanvasElement, event: &Event) {
    event.prevent_default();
    let Some(d) = event.dyn_ref::<DragEvent>() else {
        return;
    };
    let Some(list) = d.data_transfer().and_then(|dt| dt.files()) else {
        return;
    };
    let handles = files::register((0..list.length()).filter_map(|i| list.get(i)));
    if handles.is_empty() {
        return;
    }
    let p = dom_pointer(d);
    let pos = inner
        .translator
        .logical(p.client_x, p.client_y, canvas_box(canvas));
    inner.input(RawKind::Drop {
        pos,
        payload: DropPayload::Files(handles),
    });
}
