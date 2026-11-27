# Engine bridge and telemetry

The editor runs on the main thread; the audio engine runs on the audio
thread (natively) or in an `AudioWorklet` (on the web). The bridge
carries the model's changes to the engine and the host's changes back,
without locks or allocation on the audio side. Telemetry carries what the
engine renders back to the editor for scopes, meters and engine-driven
widgets.

All of this lives in `silkscreen-bridge`. Engine code depends only on
this crate (plus whatever DSP code the plugin brings), so an engine never
sees the model, the editor or a transport's internals. Without its
default `std` feature the crate is `no_std` and offers only the protocol:
`EngineEvent`, the codec, `EngineLink`, `Engine`, `NoteEvent`,
`Transport` and `DenormalGuard`.

How the model produces these events is in [model.md](model.md). How
each host drives an engine is in [hosts.md](hosts.md).

## The protocol: `EngineEvent`

`EngineEvent` is a `Copy` enum of what the editor tells the engine, in
order:

- `ParamValue { index, value }`: a plain `f32` value for a param.
- `GestureBegin { index }` and `GestureEnd { index }`: brackets around a
  user edit, which the engine forwards to the host for automation
  recording.
- `Shown { group, page }`: the editor shows `page` of page group `group`
  (see [Shown page](#shown-page)).
- `PatchLoad`: a whole patch replaces the current one (see
  [Patch loads](#patch-loads)).

The bridge addresses params by dense `ParamIndex`, not by host ids.
Mapping to stable CLAP param ids is the CLAP layer's job, so ids stay
stable while the table changes. Values are `f32` everywhere; CLAP's
`f64` values are narrowed at the CLAP boundary.

### The record codec

Every event encodes to one 16-byte record (`silkscreen_bridge::codec`):

```text
offset  size  field
     0     1  tag        1 ParamValue, 2 GestureBegin, 3 GestureEnd,
                         4 Shown, 5 PatchLoad (0 is never valid)
     1     3  reserved   written as 0, ignored on read
     4     4  index      u32 LE (Shown: the page group)
     8     4  value      f32 bits LE (0 for gestures; Shown: the page)
    12     4  reserved   written as 0, ignored on read
```

`codec::encode` and `codec::decode` round-trip every variant and keep
value bits exactly, NaN payloads included. An unknown tag is a
`DecodeError::UnknownTag`. Native transports carry the enum directly; the
codec is for transports between separately compiled modules, which on the
web means the UI and engine wasm modules. The layout is a versioned
contract: change it only by bumping `codec::VERSION` (currently 3).

## Channels

Each channel has a single writer.

| Channel | Writer → reader | Native | Web |
|---|---|---|---|
| UI events (values and gestures, in order) | main → audio | `rtrb` ring of `EngineEvent` | ring of 16-byte records in a `SharedArrayBuffer` |
| UI value slots (resync and load source) | main → audio | `[AtomicU32]` | words in the same buffer |
| Host value slots and dirty bitset | audio → main | `[AtomicU32]` and `AtomicU64` words | words in the same buffer |
| Telemetry | audio → main | `[AtomicU32]` block | words in the same buffer |
| MIDI from the page | page → audio | none (the host delivers notes) | ring of MIDI 1.0 words in the same buffer |

UI values go on the ring because their order matters relative to the
gesture brackets, and because the ring is how the engine learns what to
report to the host. `EngineLink::send` writes the value slot as well, so
the latest UI value of every param is always recoverable.

`RING_CAPACITY` (1024 events) is the default ring size: far more than a
drag sends during an audio thread's slowest block.

### Overflow and resync

A send that finds the ring full sets a sticky `resync` flag, counts the
overflow and drops the event. On its next drain the engine end discards
the queue, delivers every UI slot as a `ParamValue` and clears the flag.
Values are restored; gesture brackets in the discarded stretch are lost,
so `drain` returns `true` and the CLAP layer closes any gesture it still
has open.

### No echo

The engine publishes into the host slots only values it got from the
host (CLAP input param events, or the web page standing in for a host
with `WebEngine::host_set_param`), with `EngineEnd::publish_host`. It
never publishes values it got from the UI ring. If it did, a stale echo
(0.5 arriving after the model has moved to 0.6) would snap a knob
backwards mid-drag.

The main thread polls with `EngineLink::poll_host`, which swaps out the
dirty bitset and reports every dirty param with its latest host value;
`EditorState::pump` turns each into a `HostEvent::ParamChanged`. The
dirty bit alone is the signal. Diffing against a last-seen copy would
swallow a host value that repeats after the UI has moved the param;
the model already ignores sets to the current value.

## The seams

### The editor's end: `EngineLink`

```rust
pub trait EngineLink {
    fn send(&mut self, event: EngineEvent);
    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32));
    fn load(&mut self, values: &[f32]);
    fn telemetry(&mut self) -> Option<&mut TelemetryReader> { None }
}
```

Called on the main thread only. `send` never blocks: a transport that
runs out of room degrades to a resync, never to lost state.
Implementations:

`UiLink` (native, from `bridge(initial, ring_capacity)`, where
`initial` seeds both ends), `SabUi` (the web's UI side), `NullLink` (no
engine, as in the widgets example: a flat scope, empty meters) and
`RecordingLink` (records sends and replays host changes, for tests).

### The engine's end: `EngineEnd`, `SabEngine` and `EngineSide`

The engine end is audio-thread only, and nothing on it allocates or
blocks. `EngineEnd` (native) and `SabEngine` (web) offer:

`take_load(apply)` (a pending state load, called before `drain`),
`drain(deliver)` (pending UI events in order, handling a resync),
`publish_host(index, value)`, `read(index)` (the latest value from
either source), `seed(index)` (preferring a pending load's value) and
`telemetry()` (the `TelemetryTap` for each block).

The `EngineSide` trait collects the steps every engine host takes, so
hosts share them: `seed_engine` sets every param from `seed` and then
calls `Engine::reset`, which is what a host does at activation;
`apply_ui` hands the engine a pending load (marked as a patch load) and
then the UI's events, dropping gesture brackets, for hosts with no
automation to record (standalone and the browser). The CLAP layer drains
itself, because it also forwards values and gestures to the host.

### The engine: `Engine`

```rust
pub trait Engine: Send + 'static {
    fn set_param(&mut self, index: ParamIndex, value: f32);
    fn note(&mut self, event: NoteEvent) {}
    fn transport(&mut self, transport: &Transport) {}
    fn process(&mut self, channels: &mut [&mut [f32]]);
    fn meters(&mut self, report: &mut dyn FnMut(usize, [f32; 2])) {}
    fn values(&mut self, report: &mut dyn FnMut(usize, f32)) {}
    fn shown(&mut self, group: u16, page: u16) {}
    fn patch_load(&mut self) {}
    fn scope(&mut self, write: &mut dyn FnMut(&[f32])) {}
    fn reset(&mut self) {}
}
```

Every method must be real-time safe: no allocation, locking or I/O.

- `set_param` receives a plain value from either the UI or the host.
- `process` renders one block in place. The channels arrive filled with
  the input for effects, or zeros for instruments, all the same length.
- `note` receives a `NoteEvent` at its sample offset; hosts split
  `process` calls there. `NoteEvent` is `On`, `Off` (velocity `0..=1`;
  a CLAP note choke is an off with velocity 0), `PitchBend` (`-1..=1`
  plus the raw 14-bit value) or `Midi` for anything else, raw.
  `NoteEvent::from_midi` decodes MIDI 1.0, treating note-on with velocity
  0 as note-off. Notes never cross the UI ↔ engine channels or touch the
  model; on the web the page sends MIDI words through the buffer's MIDI
  ring, decoded with the same function.
- `transport` receives the host's `Transport` (tempo, playing, song
  position in beats, time signature) once per block before any audio is
  rendered. A missing transport, or any field the host leaves out, falls
  back per field to `Transport::default()`: 120 BPM, stopped, beat 0,
  4/4. Transport changes inside a block are ignored. On the web,
  `WebEngine` supplies a transport every block, stopped at beat 0 in 4/4,
  whose tempo the page sets (`WebEngine::set_tempo`, exported as
  `silkscreen_engine_set_tempo`); see [hosts.md](hosts.md).
- `reset` clears processing state and snaps smoothed params to their
  targets. Hosts call it after seeding a new engine.
- `meters`, `values` and `scope` feed telemetry, and `shown` and
  `patch_load` receive the matching events; all are described below.

## State loads

A host state load (CLAP `state.load`) reaches the model as
`HostEvent::StateLoaded`, applied with `Origin::Host`. The engine cursor
skips host-origin events, but unlike automation the engine doesn't have
these values yet. `EditorState::handle_host` therefore follows the load
with `EngineLink::load(values)`, every param in table order.

- `load` writes the UI value slots and sets a sticky `load` flag. The
  engine's `take_load` clears the flag, discards the queue and applies
  every slot. It doesn't deliver them through `drain`, so the CLAP layer
  sends nothing to the host for them: the host chose this state, and
  output param events could be recorded as automation.
- Gesture brackets in the discarded stretch are lost, so the CLAP layer
  closes open gestures after a load, as after a resync. UI values sent
  between the load and the engine's next drain are kept by the slots and
  applied as part of the load; that window is one block.
- A fresh engine seeded with `seed` starts at a pending load's values, so
  a project that loads state while inactive and then activates starts at
  the loaded state instead of gliding to it.

A state load is not sent as a burst of bracketed edits. That would reach
the host as user gestures, recording automation in write modes, and a
large table could overflow the ring into an ordinary resync.

## Patch loads

An engine that would otherwise retune ringing tails when a whole patch
is swapped in (for example by fading its output out and resetting) needs
to know the values that follow belong to one patch. `EngineEvent::PatchLoad`
marks that, and `Engine::patch_load` (a no-op by default) receives it
before the patch's values, all of which arrive before the next `process`.

- **A preset load** (`EditorState::load_preset`, which random sounds and
  imports also go through) sends `PatchLoad` on the ring ahead of its
  gestures, so ordering needs no flag.
- **A state load** is marked by the engine end: hosts pass
  `patch_loaded(to_engine)` as `take_load`'s callback, which sends
  `PatchLoad` ahead of the first value.
- The host never hears of a patch load.
- An overflow resync discards the queue, mark included, and the values
  then arrive unmarked.
- Undoing a preset load is an ordinary edit and is not marked.

## The web transport

On the web the UI and the engine are separate wasm modules that can't
share linear memory, so every channel lives in one `SharedArrayBuffer`
of 32-bit words, reached through the `SharedWords` trait: JS `Atomics`
over an `Int32Array` in the browser, `[AtomicU32]` in native tests.
`SabUi` and `SabEngine` are the same code on every target, so the web
transport is tested natively.

A header (magic `0x5353_4142`, "SSAB"; the layout version,
`sab::VERSION`; sizes, cursors and flags) is followed by the ring, the UI
slots, the host slots, the dirty bitset, the telemetry block and the
MIDI ring; the full layout is documented in
`crates/silkscreen-bridge/src/sab.rs`. `SabLayout::read` refuses a
buffer whose magic, version or sizes don't match. Like the codec, the
layout is a versioned contract between two modules.

`WebEngine` owns an engine, its `SabEngine` and one 128-frame quantum of
audio per channel (at most two). `WebEngine::new` attaches and seeds the
engine; each `render` applies UI events, renders and writes telemetry.
The `export_web_engine!` macro wraps one in the C-ABI exports the
worklet calls. See [hosts.md](hosts.md).

## Floating-point mode

Subnormal arithmetic can cost up to about 100× on x86 and shows up in
decaying feedback state: filters, envelope tails, delay lines. Engines
don't check for it. Instead, the host holds a `DenormalGuard` for each
process call and the CPU flushes subnormals for the engine's whole
render.

- **Per call, restored on drop.** The control word belongs to a
  thread, and a host may call `process` on a different thread each time;
  restoring it leaves the host and later plugins their own mode.
- **Inputs too.** On x86_64 the guard sets MXCSR FTZ and DAZ; FTZ alone
  still pays for a subnormal that arrives as an input. On aarch64 it sets
  FPCR FZ, which covers inputs and results. ARM64EC code (Windows on Arm
  hosts that load x64 or ARM64EC plugins) runs on the same FPCR and does
  the same.
- **wasm** has no floating-point control word, so the guard is a no-op
  there.
- The CLAP processor and the standalone runner take the guard
  themselves, so a plugin can't forget it.

Engine code therefore needs no ad hoc flushes: no tiny DC offsets and no
`|x| < 1e-30` tests. A threshold the instrument defines for itself, such
as an envelope's end-of-tail level, is behaviour and stays.

## Telemetry

Telemetry bypasses the model entirely. It is written on the audio thread
after `process` and read by the editor on each `tick`.

### The block

`telemetry()` returns a `TelemetryTap` (audio thread) and a
`TelemetryReader` (main thread) over one block of 32-bit words. Natively
the block is its own `Arc<[AtomicU32]>`, created inside `bridge`; on the
web it is a region of the shared buffer, the same code over `Atomics`.
In words from the block's base:

```text
word   field
   0   written        samples written, mod 2^32
   1   channels       output channels metered
   2   peaks          MAX_CHANNELS (2) × f32: the output's peaks
   4   engine peaks   ENGINE_METERS (4) × 2 × f32: the engine's meters
  12   values         ENGINE_VALUES (16) × f32: the engine's values
  28   scope          SCOPE_CAPACITY (4096) × f32: a mono ring
4124   frame seq      frames published, mod 2^32; 0 for none
4125   frame cap      F, the frame's capacity in words
4126   frames         2 × (1 + F): two buffers, each a length and F × f32
```

A block with no frame (F = 0) is `TELEMETRY_WORDS` long; one with a
frame is `telemetry_words(F)`.

- **Scope:** the writer stores samples, then `written` with release
  ordering; the reader loads `written` with acquire and copies the latest
  stretch, oldest first. Each sample is whole, but a copy that races the
  writer may mix in what was written during it; the next frame replaces
  it.
- **Peaks** hold the largest `|sample|` since the reader last took them.
  The writer raises them with `fetch_max` on the bits, which orders the
  same as the value for non-negative floats; the reader takes them with
  `swap(0)`.
- **Values** are stored whole and read whole: each is the latest the
  engine wrote.
- **The frame** is a seqlock over two buffers. The tap writes frame `n`
  into buffer `n & 1`, its length first, then stores `n` as the frame
  seq (release). The reader loads the seq (acquire), copies that buffer
  a word at a time (acquire loads, not a bulk copy, so the copy is
  ordered before the check), then loads the seq again. If it moved, the
  tap may have begun the next frame but one in the buffer under the
  copy, so the copy is thrown away and tried again, up to four times;
  failing that, the reader keeps the frame it has until the next tick.
  The tap never waits, and the editor never sees a frame that mixes two.

The tap never allocates, locks or blocks.

### Writing a block

Every host calls `TelemetryTap::write_block(engine, channels)` once per
block, after `process`. It records:

- the scope: the engine's own samples if `Engine::scope` wrote any
  (mono, at the host's rate, in one or more runs), else the mono sum of
  the output, `(L + R) / 2` for stereo;
- the output's peaks per channel;
- the engine's meters, from `Engine::meters`: `report(n, [left, right])`
  with meter `n`'s peak over the block, for up to `ENGINE_METERS`;
- the engine's values, from `Engine::values`: `report(n, value)` for up
  to `ENGINE_VALUES` floats, such as an envelope's level, an LFO's phase
  or the number of voices sounding.
- the engine's frame, from `Engine::frame`: `write(frame)` with a
  snapshot of values that belong together, such as every voice's
  position and gain, which the editor must not see half of. Up to the
  frame's capacity; words past it are dropped.

So every engine gets an output scope and meter for free, and can supply
signals only it can see when an editor needs them.

### Frame size

A plugin declares its frame's size in words at build time, as
`PluginUi::frame` (0 for none). Every host makes room for it: the CLAP
layer with `bridge_with_frame`, the web page with
`SabLayout::with_frame`, which records it in the shared buffer's
telemetry size, so the worklet's `SabEngine::attach` reads it back. A
frame can be at most `MAX_FRAME` (65536) words. Frames are for a display
that draws many values at once; a handful of independent readings
belong in `Engine::values`.

### Reading on `tick`

On each tick the editor takes the peaks, copies the scope ring if new
samples arrived (no more often than `ScopeStyle::refresh_ms`), and reads
the engine values and the latest frame. Each feed keeps a generation that moves on when what
it shows changes, so a control is damaged only when its drawing would
change. This is the compositor's time-driven damage (see
[rendering.md](rendering.md)).

### Scope and meter controls

Two display-only control kinds, bound with `Unbound`, draw telemetry
from `silkscreen_core::telemetry`. Their widgets read the editor's feeds,
not `WidgetState`, and they take no input and show no tooltip.

- **`scope.<name>`** (`Scope`) draws a `SCOPE_WINDOW` (1024-sample)
  window starting at the latest rising zero crossing that leaves a whole
  window after it, armed only once the signal has gone below
  `-hysteresis`, so noise doesn't trigger it. With no such crossing
  (silence, a slow sweep) it draws the latest window. `ScopeStyle` sets
  full scale, hysteresis, colour and width, an optional axis line drawn
  over the wave, and the least time between redraws; a slower refresh
  reads steadier.
- **`meter.<name>`** (`Meter`) draws one segmented bar per channel on a
  dBFS scale. `MeterStyle::default()` runs from −48 to +6 dBFS in 27
  segments, green to −18, blending through amber at −6 to red at 0 and
  over; unlit segments are a dim tint of their lit colour. `Ballistics`
  turns peaks into a `MeterReading`: the bar jumps to a peak and falls at
  `fall_db_per_s`; the hold tick stays for `hold_ms` (1.5 s) and then
  falls at `hold_fall_db_per_s`.

Set the styles with `EditorState::set_scope_style` and
`set_meter_style` before a view is made. A meter shows the output unless
pointed elsewhere with `EditorState::set_meter_source(control,
MeterSource::Engine(n))`; each meter keeps its own ballistics.

### Feeds for plugin widgets

A plugin drawing its own display reads the same feeds, each with a
generation to repaint on:

`EditorState::scope_trace()` (the built-in scope's triggered window),
`scope_samples()` (the whole ring, untriggered, for a scope that
triggers its own way), `meter_reading(control)`, `engine_values()` and
`engine_frame()` (the latest complete frame; its generation moves on
with each new one).

`LevelCommand` also reads the engine meters: its press sets a gain param
so meter `n`'s latest peak, measured before that gain, lands at a target
dBFS (`EditorState::match_level`). It declines below −60 dBFS or without
a reading.

### Shown page

`EngineEvent::Shown { group, page }` tells the engine which page of each
page group the editor shows. `EditorState::new` sends every group's page
when it is made, and the editor sends again on every change; hosts hand
it to `Engine::shown`. It lets an engine's telemetry follow the page: a
scope that shows the layer on screen, say, by capturing only that layer
in `Engine::scope`.

The shown page is view state. It is never a param, never saved with a
preset, never sent to the host as automation and never undone. A resync
doesn't repeat it: the engine keeps the last page it heard of until the
next change. The CLAP layer remembers the last page per group and
replays it to an engine created at a later activation.

In tests, `silkscreen-harness` fills the telemetry block with
`Harness::output`, `engine_meter`, `engine_value` and `engine_frame`
(up to `Harness::FRAME` words) and steps `now_ms`,
so scopes and meters snapshot like any other control.
