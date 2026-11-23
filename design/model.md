# Model, parameters and bindings

Every change to a plugin's state has one arbiter. The user (through the
editor) and the host (through automation, its generic parameter UI and
state restore) both drive change, and both take the same path into one
model. This document covers that path, how parameters are described, and
the bindings that turn input on a control into edits. The types live in
`silkscreen-model`, which builds for `wasm32-unknown-unknown` and knows
nothing of transports or engines. How the model's changes reach the audio
engine is in [engine-bridge.md](engine-bridge.md).

## The flow

```text
RawEvent ─► GestureRecognizer ─► ControlEvent ─► Controller ─► Binding ─► Edit ─► Model
HostEvent ─────────────────────────────────────► Controller ─────────────────────► Model
Model ─► EventLog ─► view cursor   ─► damage / redraw
                  ─► engine cursor ─► EngineLink
```

Three steps are kept apart:

- **"This was clicked."** Input becomes a `ControlEvent` for a
  `ControlId` (see [input.md](input.md)). The `Controller` holds one
  `Binding` per control and hands the event to that control's binding.
- **"This was changed."** The binding edits the model through an `Edit`:
  `begin`, `set`, `set_normalized`, `end`, `reset`. Bindings hold the
  interaction logic (drag sensitivity, Shift for fine adjustment, reset).
  Widgets only draw.
- **"This event occurred."** Each change appends a `ModelEvent` to the
  model's `EventLog`. The view and the engine each read the log through
  their own `Cursor`. Control events never reach the engine directly.

`EditorState` (in the `silkscreen` crate) owns the model, the controller,
the engine link and both cursors. The log is append-only with sequence
numbers, and `EventLog::compact` drops what every cursor has read.

## The model

`Model::new(specs)` takes a `&'static [ParamSpec]` table, indexed by a
dense `ParamIndex(u32)`, and starts every param at its default.

- Values are stored **plain**, in the param's own units, as `f32`.
  Bindings and widgets work in normalized `[0, 1]` through the spec's
  `normalize` and `denormalize`.
- Every value the model accepts passes through `ParamSpec::clamp`, so it
  is always in range, and on a whole position for discrete kinds.
- Setting a param to its current value (bit for bit) logs nothing.
- `ModelEvent` is `ParamChanged { param, value, origin }`,
  `GestureBegin { param, origin }` or `GestureEnd { param, origin }`.
- `Origin` is `Ui`, `Host` or `Undo`.

The only way to change state is `Model::edit(origin)`, which returns an
`Edit` attributed to one origin. Bindings receive one with `Origin::Ui`.

### Requests to the view

The model has no UI, so a binding that needs the view to do something
other than change a value queues a `UiRequest` with `Edit::request`. The
editor drains them with `Model::take_requests` and acts: open a menu of a
stepped param's choices (`Menu`), open the preset browser (`Presets`) or
a plugin panel (`Panel`), show a tab's page (`ShowPage`), step, save,
save as, init or randomise the preset (`PresetStep`, `PresetSave`,
`PresetSaveAs`, `PresetInit`, `PresetRandomise`, see
[presets.md](presets.md)), or set a gain so an engine meter's peak lands
at a target level (`MatchLevel`).

## The host as a second controller input

Host changes arrive as `HostEvent`s through `Controller::handle_host`
(`EditorState::handle_host` and `EditorState::pump` call it). No binding
is involved; they are applied with `Origin::Host`.

- `HostEvent::ParamChanged { param, value }` sets one param. If the user
  is mid-gesture on the same param, the last writer wins.
- `HostEvent::StateLoaded(values)` carries `(ParamIndex, value)` pairs
  for a whole saved state. Each listed param takes its value and every
  other param returns to its default. Non-finite values count as not
  listed, since they can't be clamped into range.

The view redraws for host changes like any other. The engine cursor
skips `Host`-origin events: the engine already has those values, and
echoing them back would fight edits the user has in flight. That filter,
in `EditorState`'s engine flush, is the only place echo suppression
lives. A state load is the exception to "the engine already has them",
so `EditorState::handle_host` follows a `StateLoaded` with a separate
load request to the engine (see [engine-bridge.md](engine-bridge.md)).

## Gestures

`begin` and `end` bracket every user edit: a drag, a wheel step, a
click on a switch, a reset, an undo step. The brackets travel to the
engine, which forwards them to the host (CLAP `begin_gesture` and
`end_gesture`) so the host can record automation.

- `begin` on a param whose gesture is already open is a no-op.
- `end` without a matching `begin` is a caller bug: it asserts in debug
  builds and is ignored in release.
- `Edit::reset(p)` is a complete gesture that sets the default.

## Undo

The model keeps undo history from completed gestures.

- When a `Ui`-origin gesture ends with a different value from the one it
  began with, it becomes an `UndoEntry { param, before, after }`.
  Gestures with no net change are dropped. Host-origin changes are never
  undoable.
- `Edit::group(f)` collects every gesture `f` completes into one undo
  step. Loading a preset uses this, so the whole load undoes at once.
- `Model::undo` and `Model::redo` replay a step with `Origin::Undo`,
  each param inside its own begin/end bracket, so the host records undo
  as automation too. Both return `false` with nothing to do, and both
  are refused while any gesture is open, so they can't interleave with a
  drag.
- A new user edit clears the redo stack.
- A host state load clears both stacks (`Model::clear_history`): their
  entries describe edits to the state the load replaced. A user gesture
  open across the load is re-based to the loaded value, so ending it
  records only what changed after the load.
- The stack keeps 100 steps by default; `Model::set_undo_depth` changes
  that.

Hosts call `EditorState::undo` and `EditorState::redo` (the native host
maps its Undo and Redo commands to them), which also flush the replayed
events to the engine.

## Parameter specs

A `ParamSpec` is a `const`-constructible description of one parameter:
`name`, `min`, `max`, `default`, `taper`, `kind`, `unit`, `decimals`,
an optional `display` mapping, a stable host `id`, and `automatable`.

```rust
static PARAMS: [ParamSpec; 3] = [
    ParamSpec::new("Gain", -60.0, 12.0, 0.0).unit("dB").decimals(1).id(1),
    ParamSpec::stepped("Wave", &["Sine", "Saw", "Pulse"], 0).id(2),
    ParamSpec::toggle("Bypass", false).id(3),
];
```

- **`id`** is what hosts key saved projects and automation on, and what
  presets are keyed by. It is independent of the param's position in
  the table, so the table can be reordered. It is `ParamSpec::NO_ID`
  until set. Never change or reuse an id once a plugin has shipped.
- **`unit` and `decimals`** control `format`: `"-6.0 dB"`.
- **`parse`** reads text such as `"-6"`, `"-6.0 dB"` or `" +3dB"`: a
  sign, a decimal number and any trailing unit. The result is clamped.

### Kinds

`ParamKind` says what values a param takes:

- **`Continuous`**: any value in `min..=max`, shaped by the taper.
  `ParamSpec::new` builds one.
- **`Stepped { labels }`**: whole positions `0..=n − 1`, one per label.
  `ParamSpec::stepped(name, labels, default_index)` sets `min = 0` and
  `max = n − 1`. `clamp`, `normalize` and `denormalize` all snap to a
  position, so automation can't land between positions. `format`
  returns the label, and `parse` accepts a label (ignoring case) or a
  number.
- **`Toggle { labels }`**: off (`0`) or on (`1`). `ParamSpec::toggle(name,
  default_on)` builds one, and `.labels(&["Down", "Up"])` names the two
  states, off first. Without labels it shows "Off" and "On".

Discrete values are exact small integers in `f32` everywhere, including
the bridge, so an engine reads them with an `as usize` cast.
`ParamSpec::position_labels` gives the labels of either discrete kind.

In CLAP, stepped and toggle params are flagged `IS_STEPPED` and
`IS_ENUM` with a range of `0..=n − 1`. Both always have labels, so hosts
that show enums as a list of names can, and the CLAP text conversions
use the labels.

### Tapers

`Taper` shapes how a continuous param's plain range maps to `[0, 1]`.
It moves widgets only: the model, the bridge and host automation stay
plain.

- **`Linear`**, the default.
- **`Bipolar`**: linear, centred. `.bipolar()` sets it and moves the
  default to the middle of the range. Widgets draw the value arc from the
  centre (`WidgetState::origin = 0.5`).
- **`Exp { mid }`**: the middle of travel reads `mid` and the top `max`.
  Geometric on each side of `mid` when `0 < min < mid < max`; otherwise
  one exponential through `(0, 0)`, `(0.5, mid)` and `(1, max)`. For
  cutoffs and times.
- **`BipolarExp { mid }`**: `Exp` mirrored about the centre of a
  symmetric range `-max..=max`, so half travel each way reads `±mid`.
  Drawn from the centre. With `mid ≥ max / 2` there is no rising curve
  through those points, and it maps linearly.

`ParamSpec::is_bipolar` is true for `Bipolar` and `BipolarExp`.

### Display units

A continuous param can hold one thing and show another, for example a
slider position `0..1` that the host's generic UI should show in hertz.
`.display(to_display, from_display)` gives the spec a pair of plain
`fn(f32) -> f32` (the `Display` struct):

- `format` shows `to_display(clamp(plain))` with the spec's unit and
  decimals.
- `parse` reads the number, maps it through `from_display` and clamps. A
  number that maps to no plain value (NaN, infinite) is refused.
- A display may reach ±∞ at an end of its range (a gain of zero in
  decibels). `format` shows `inf` and `-inf`, and `parse` takes those
  back through `from_display`, so every shown value parses.

The engine owns the conversion laws: the plugin's mappings should call
the same formulas the engine uses, and a test should hold each pair to a
round trip. `Display` compares by function address
(`std::ptr::fn_addr_eq`), which is enough for `ParamSpec: PartialEq`.

### Patch state

State that isn't a performance control, such as a modulation matrix's
routing, is held in ordinary params marked `.patch_state()`, which sets
`automatable` to false. Save and load, presets, undo, the engine bridge,
bindings and panels then need nothing new. The CLAP layer marks these
params `IS_HIDDEN` without `IS_AUTOMATABLE`, so they stay out of
automation lists, but their values still travel in the plugin's state.
Values a user should be able to automate stay ordinary params.

## Bindings

```rust
pub trait Binding {
    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>);
    fn display(&self, model: &Model) -> WidgetState;
    fn params(&self) -> &[ParamIndex];
    fn describe(&self, model: &Model) -> Option<String> { .. }
    fn cursor(&self) -> Option<MouseCursor> { .. }
    fn attach(&mut self, control: &ControlSpec, specs: &[ParamSpec], layout: &Layout)
        -> Result<(), String> { .. }
}
```

- `display` says what the widget draws: a normalized value, a position
  count for detents, an arc origin and a dimmed flag. The view fills in
  hover, press and focus.
- `params` lists the params `display` reads, so the view knows which
  controls to repaint when the log reports a change.
- `describe` overrides the tooltip, which by default is the first
  param's name and formatted value.
- `attach` runs once, when `EditorState::new` builds the editor. Built-in
  bindings check that the control kind and param kind suit them and keep
  any geometry they need (a fader's track). `EditorState::new` also
  checks that every param a binding names exists. Any failure panics
  there, naming the control by its SVG id, the param and the fix. Custom
  bindings accept anything by default.

The generated `Controls` struct has one field per control on the
faceplate (see [faceplates.md](faceplates.md)); its `into_array` yields
the bindings in `ControlId` order for `EditorState::new`.

### Built-in bindings

| Binding | Control | Param | Behaviour |
|---|---|---|---|
| `ParamKnob` | knob | Continuous or Stepped | Vertical drag, 200 logical px for full range; Shift for ×0.1; double-click or Cmd-click resets; wheel 1% per line, or one position for a stepped param |
| `ParamFader` | fader | Continuous, or Stepped with many positions | Drag moves 1:1 with the cap; a click on the track jumps there; Shift, reset and wheel as the knob |
| `ParamSelector` | selector, or a three-position toggle | Stepped | Drag snaps between detents; a click picks the nearest position; wheel steps one |
| `ParamToggle` | toggle, button | Toggle | A click flips it, as one gesture |
| `ParamRadio` | `radio.<group>.<n>` | Stepped | A click sets the group's param to `n` |
| `ParamDropDown` | dropdown | Stepped | A click asks for a menu of the choices; choosing commits one gesture; wheel steps |
| `SyncedKnob` | knob | Continuous time, Stepped division, Toggle sync | Drives the free time param, or with sync on, the division param, stepping shortest to longest; a drag stays with the param it began on |
| `PanelButton` | button or action | none (reads any) | Opens the plugin's panel for the control; its lamp lights while a predicate holds |
| `TabButton` | tab | Toggle lamp, or none | Shows its page; `.lamp(param)` lights it while a toggle is on |
| `PresetButton` | preset | none | Opens the preset browser, or steps with the ‹ and › at its ends |
| `PresetCommand` | action | none | Save, Save As, Init, Randomise, Previous, Next or Browse |
| `LevelCommand` | action | Continuous gain | Sets a linear gain param so an engine meter's slow peak lands at a target dBFS, as one undo step |
| `Unbound` | scope, meter, canvas | none | Display only: no input, no tooltip |

Every built-in edit is bracketed by a gesture. Several radio buttons
sharing one param, or a knob and a fader mirroring each other, work
because the view maps each param to every control that reads it.

A plugin opens panels of its own by registering a maker with
`EditorState::set_panel` for a control bound with `PanelButton` (or any
binding that posts `UiRequest::Panel`). Panels are covered in
[overlays-and-text.md](overlays-and-text.md).

### Custom bindings and adapters

Any type implementing `Binding` can be used. For small cases,
`binding::from_fn(params, on_event, display)` builds one from closures.
`Box<dyn Binding>` is itself a `Binding`, so boxed bindings can still be
wrapped.

`BindingExt` adapts any binding:

- **`describe_with(f)`** returns a `Described` binding whose tooltip and
  drag readout come from `f: fn(&Model) -> Option<String>`. Use it for a
  control whose readout depends on other params, or one that stands for
  several params or none.
- **`dimmed_when(inputs, predicate)`** returns a `Dimmed` binding whose
  control is drawn faded while `predicate(&Model)` holds. `inputs` lists
  the params the predicate reads, so the control repaints when any of
  them changes. Use it for a control that does nothing in the current
  patch, such as a legato switch while the synth is in a polyphonic
  mode.

### Dimmed controls

A dimmed control still works: it takes input and edits its param as
usual. Only its drawing changes. `Dimmed::display` sets
`WidgetState::dimmed`, and the compositor draws the widget at
`DIM_OPACITY` (0.4) over what lay beneath it on the faceplate, so widgets
need no code for it. A widget that shows dimming its own way (a sprite
fader with a dark cap, say) returns `true` from `Widget::draws_dimmed`
and is drawn at full opacity with the flag set. See
[rendering.md](rendering.md).
