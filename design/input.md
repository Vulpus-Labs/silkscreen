# Input and gestures

Hosts deliver input in different shapes: baseview events natively, DOM
events in the browser, scripted steps in the test harness. silkscreen
turns all of them into one vocabulary of raw events, and one recognizer
turns those into control-level gestures. Host crates only translate;
gesture logic and its tests exist once.

## Raw events

`silkscreen_core::RawEvent` is the host-neutral input type:

```rust
pub struct RawEvent { pub time_ms: f64, pub kind: RawKind }

pub enum RawKind {
    PointerDown { pos: Point, button: MouseButton, mods: Modifiers },
    PointerUp { pos: Point, button: MouseButton, mods: Modifiers },
    PointerMove { pos: Point, mods: Modifiers },
    PointerLeave,
    Wheel { pos: Point, delta: Vec2, mods: Modifiers },
    Key { key: Key, mods: Modifiers, pressed: bool },
    FocusLost,
    Drop { pos: Point, payload: DropPayload },
}
```

- **Positions are logical.** The host divides physical coordinates by the
  effective scale (see [rendering.md](rendering.md)).
- **Time is the host's.** `time_ms` is milliseconds on any monotonic
  clock the host chooses; only differences matter. The native host uses
  one clock origin per process, so time never runs backwards when an
  editor is closed and reopened.
- **Modifiers** hold `shift`, `alt`, `ctrl` and `cmd`. `cmd` is the
  platform's primary modifier: Cmd on macOS, Ctrl elsewhere (where it is
  set together with `ctrl`). `Modifiers::from_flags` builds them from the
  four flags hosts report. Bindings test `cmd` for shortcuts such as
  reset to default.
- **Wheel deltas are in lines**, positive `y` scrolling up. Hosts that
  report pixel-precise scrolling convert with `PIXELS_PER_LINE` (16
  device-independent pixels per line).
- **Keys** are the few silkscreen cares about (`Key::Char`, arrows,
  Enter, Escape, Backspace and so on). `Key::from_name` reads the DOM's
  `KeyboardEvent.key` names, which `keyboard-types` uses too.
- **Drops** carry a `DropPayload`: `Paths(Vec<PathBuf>)` on native hosts,
  and `Files(Vec<u32>)` on the web, handles to the browser `File`s of one
  drop. The web host resolves a handle with
  `silkscreen_host_web::take_dropped_file` while the drop is being
  handled; the next drop releases any not taken.

## The gesture recognizer

`GestureRecognizer` in `silkscreen-core` is a pure state machine. Time
comes from event timestamps and from its `tick`, never from a clock. It
resolves raw events against the layout and emits `(ControlId,
ControlEvent)` pairs.

| Control event | When |
|---|---|
| `Press { pos, button, mods }` / `Release { .. }` | A button goes down or up on a control. |
| `Click { count, mods }` | A left press and release without crossing the drag threshold. `count` is 2 for a double-click, 3 for a triple, and so on, when each press follows the previous click on the same control within the double-click time. |
| `DragStart { pos, mods }` | Movement from the press point passes the drag threshold. `pos` is where the press started. |
| `Drag { delta, pos, mods }` | Each move while dragging. `delta` is the movement since the previous `Drag` (or since the press point, for the first). |
| `DragEnd { over, mods }` | The button is released. `over` is the control under the pointer at release, which gives control-to-control drag and drop; it is `None` if the drag was cancelled. |
| `Wheel { delta, mods }` | Scroll over a control. |
| `HoverEnter` / `HoverLeave` | The pointer, with no button held, enters or leaves a control. |
| `Hover { pos }` | The pointer moves over a control with no button held, after its `HoverEnter`. For controls with handles of their own, such as a canvas. |
| `HoverDwell` | The pointer has rested on a control for the dwell time. Fires once per rest; movement or a press cancels it. Tooltips hang off it. |
| `Key { key, mods, pressed }` | A key, sent to the focused control: the last one pressed. |
| `Drop { payload }` | Files dropped on a control. |

The thresholds are in `GestureConfig`:

| Field | Default |
|---|---|
| `drag_threshold` | 3 logical px |
| `double_click_ms` | 400 ms |
| `dwell_ms` | 500 ms |

Editors use the defaults.

Details that matter when writing bindings:

- **Only the left button clicks and drags.** Other buttons produce
  `Press` and `Release` only, so bindings can implement context actions
  on them. A second button pressed while one is held is ignored until all
  are released.
- **A drag captures the pointer.** Once a press starts, every move goes to
  the pressed control until release, even outside the window. Hosts must
  honour this: baseview's capture natively, `setPointerCapture` on the
  web.
- **Hover is frozen while a button is held** and catches up on release.
- **Losing focus ends the gesture.** `FocusLost` clears keyboard focus and
  cancels any press or drag, ending an open drag with `DragEnd { over:
  None }`, so a lost release never leaves a host parameter gesture open.
  Hosts call `EditorView::cancel` before dropping a view for the same
  reason.
- **Double-clicks come from the recognizer.** The DOM's own `dblclick` is
  never listened to, so timing is the same on every host.

## Hit testing

`Layout::hit_test_shown` walks the controls in reverse document order, so
the topmost control (the last in the SVG) wins. Display-only controls,
such as scopes and meters, are never hit, and nor are controls on pages
that aren't shown (see [faceplates.md](faceplates.md)). When the shown
page changes, a hover or focus on a control that has gone is dropped.

## Routing inside the editor

`EditorView::handle` takes each raw event and routes it, in order:

1. A preset file dropped anywhere on the editor is imported (see
   [presets.md](presets.md)).
2. If a panel is open (a menu, dialog or the preset browser), the top
   panel gets the event. A press outside it or Escape closes it, and
   the controls beneath see nothing (see
   [overlays-and-text.md](overlays-and-text.md)).
3. A press on the licence strip opens the unlock dialog (see
   [licensing-hook.md](licensing-hook.md)).
4. Everything else goes to the gesture recognizer, and its control events
   go to the control's binding (see [model.md](model.md)).

Movement, a press, a scroll or the pointer leaving hides a tooltip.

## Default conventions

The stock parameter bindings (`ParamKnob`, `ParamFader` and others in
`silkscreen-model`) share these conventions:

- Drag changes the value; Shift-drag is fine adjustment (`fine`, 0.1×
  by default), and Shift can be pressed or released mid-drag.
- Double-click or Cmd-click (Ctrl-click off macOS) resets to the default.
- The wheel steps the value (`wheel_step` per line, times `fine` with
  Shift).

Modifiers arrive with every mouse event, so these work whichever window
has the keyboard.

## Keyboard

Nothing essential depends on key input. Keys are delivered when the host
forwards them, and undo and redo have an API (`Editor::undo`,
`Editor::redo`) that hosts call from their own menus.

In a plugin the keyboard belongs to the DAW. `silkscreen-clap` opens its
window with `WindowOptions::in_host` set, which has two effects.

**Keys pass through.** The editor takes keys only while the top panel
takes typed text (`Panel::takes_text`), as the preset browser's search
field does. For every other key the native host returns
`EventStatus::Ignored`, which on macOS baseview passes up the responder
chain to the DAW's window. Plugins have no editor keyboard shortcuts, and
menus aren't keyboard-driven.

**Names are typed in a prompt window.** A DAW takes some keys (Space, for
the transport) before a plugin's view sees them, so a name can't be typed
reliably into a field drawn on the faceplate. The host gives the editor a
`TextPrompt` (`EditorState::set_text_prompt`), and name fields (rename,
new folder, Save As) ask it for their text. It opens a small window of
its own that has the keyboard, which the DAW's key handling doesn't
reach. The answer comes back through a `Prompt` and its `PromptReply`:
`Some(text)` on Enter, `None` when cancelled.

- On macOS the prompt is an `NSAlert` sheet with a text field, run with a
  completion handler: no nested modal loop and no Objective-C class
  declared.
- On Windows it is an owned popup with an `EDIT` control that handles
  Return and Escape itself.
- Linux has no prompt; fields take keys directly.

The standalone app and the web have no DAW beside them. `in_host` is off,
no prompt is set, every key goes to the editor and fields are typed into
directly.

## Web specifics

The web host translates pointer events from the canvas and maps client
coordinates through the canvas's box, so input stays correct at any page
zoom. A few behaviours are specific to the browser:

- The wheel and touch scroll the page except where they are the editor's:
  over a control that takes input, or anywhere while a panel is open.
  There the default is suppressed and the control gets the event. A wheel
  gesture the browser has already started scrolling with can't be
  cancelled, so a knob that scrolls under the pointer doesn't turn then.
- Only the primary pointer is used; extra touch points and pens beside
  the mouse are ignored.
- Right-click belongs to the editor: the context menu is suppressed.
- A press focuses the canvas, which is made focusable, so it receives
  key events. Blur sends `FocusLost`.
