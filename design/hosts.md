# Hosts

A silkscreen editor runs in three hosts: a CLAP plugin inside a DAW, a web
page, and a standalone desktop app. All three drive the same `Editor`
(see [overview.md](overview.md)). A host's whole job is to:

- feed it `RawEvent`s in logical coordinates with a millisecond timestamp,
- call `tick(now_ms)` periodically,
- report scale and size changes,
- call `render` into a pixel buffer and present the damage rects it returns.

The headless test harness (`silkscreen-harness`) implements the same
contract.

The editor's long-lived half, `EditorState` (model, controller, the UI end of
the engine bridge), outlives its view. In a plugin, the view (window,
compositor, recognizer) exists only while the host shows the editor, and
engine traffic and host automation keep working while it is closed. Each
plugin describes itself to every host with one `silkscreen::PluginUi`
(vendor, product, version, `web_prefix`, parameter `specs`, factory
presets, faceplate PNGs, and a `state` function that builds the
`EditorState` over an engine link). Hosts add what is theirs: a preset
store, a clipboard, a text prompt, and in a CLAP plugin the licence lock.

## CLAP: `silkscreen-clap`

`silkscreen-clap` is the CLAP shell, built on clack (pinned to a git
revision in the workspace `Cargo.toml`). A plugin implements
`SilkscreenPlugin` and exports itself with `export_clap!`:

```rust
use std::ffi::CStr;
use silkscreen::PluginUi;
use silkscreen_clap::{SilkscreenPlugin, export_clap, features};

pub struct GainPlugin;

impl SilkscreenPlugin for GainPlugin {
    const ID: &'static str = "com.example.gain";
    const NAME: &'static str = "Gain";
    const VERSION: &'static str = gain::UI.version;
    const FEATURES: &'static [&'static CStr] = &[features::AUDIO_EFFECT, features::STEREO];
    const AUDIO_INPUT: bool = true;
    const UI: PluginUi = gain::UI;

    type Engine = gain::engine::GainEngine;

    fn engine(sample_rate: f32, _max_frames: u32) -> Self::Engine {
        gain::engine::GainEngine::new(sample_rate)
    }
}

export_clap!(GainPlugin);
```

The crate is a `cdylib`. `ID` is the reverse-DNS plugin id. Hosts key
projects on it, so never change it after a release. The optional constants:

| Constant | Default | Meaning |
|---|---|---|
| `BUILD` | `""` | A build id shown beside the version in the editor's corner badge, e.g. the git hash `silkscreen_build::stamp_build_id()` stamps into `SILKSCREEN_BUILD_ID`. |
| `DIAGNOSTICS` | `false` | Adds host changes (`hc`), timer ticks (`tk`), tempo and play state to the badge. |
| `NOTE_INPUT` | `false` | One note input port. |
| `USER_PRESETS` | `false` | Keep user presets on disk (see [presets.md](presets.md)). |
| `LATENCY` | `0` | Samples of output delay, reported for delay compensation. |
| `LICENCE` | `None` | A code check that locks the plugin until unlocked (see [licensing-hook.md](licensing-hook.md)). |

`preset_store()` and `licence_file()` have default implementations a plugin
may override.

### Threads

The main thread owns the `EditorState` for the plugin's life. The audio
thread owns the engine and the bridge's engine end. While the plugin is
inactive the engine end waits on the main thread. On `activate` it moves
into the processor, and on `deactivate` it moves back, so nothing is lost
across reactivation. `engine(sample_rate, max_frames)` runs on the main
thread at activation, so it may allocate. A fresh engine is seeded with
every current value, including a state load not yet taken, before its first
block, so its smoothers start at those values rather than gliding to them.

### Extensions

The shell registers audio-ports, params, gui, state, note-ports, latency and
timer.

- **Audio ports.** One stereo output, and with `AUDIO_INPUT` one stereo
  input, paired in place.
- **Params**, from the `ParamSpec` table. Every spec needs an explicit,
  unique `.id(n)`. The shell refuses to create a plugin otherwise, and the
  error names the parameter. Stable CLAP ids map to dense `ParamIndex`
  values by binary search, which is safe on the audio thread. Stepped and
  toggle kinds are reported as stepped enums. A spec that isn't
  `automatable` is hidden. `value_to_text` and `text_to_value` go through
  the spec's own format and parse. `get_value` reads the model. `flush`
  works on whichever thread the host calls it from.
- **Process.** Each call holds a `DenormalGuard`, drains the UI ring
  (applying values to the engine and emitting `ParamValue` output events
  between gesture begin and end), converts the host transport to a
  `Transport` for the engine, and applies host events sample-accurately in
  batches. A host value reaches the engine and is published back to the
  model. If the UI sends something while the audio thread isn't running,
  the shell asks the host for a `flush`.
- **Note ports.** With `NOTE_INPUT`, one input port speaking the CLAP and
  MIDI dialects, CLAP preferred. Note on, note off, choke (treated as off)
  and MIDI channel messages are decoded to `NoteEvent` for `Engine::note`.
  Notes never touch the model. Wildcard keys and channels are ignored.
- **State.** Saved bytes start with the magic `SLKS` and a format version
  (currently 3), followed by values keyed by CLAP id, the editor zoom, and
  the editor's UI state as key-value pairs (such as each page group's shown
  page). The layout is documented in `silkscreen-clap/src/state.rs` and is a
  compatibility contract: change it only by bumping the version and keep
  loading every older one. Load ignores unknown ids, and refuses bad magic,
  truncation and newer versions. A load posts `HostEvent::StateLoaded`, the
  engine resyncs through the bridge, and the shell ends with a values
  rescan so the host re-reads every parameter.
- **GUI.** The editor is a baseview child window of the host's window,
  presented with softbuffer (`silkscreen_host_native::open_embedded`).
  Floating windows aren't supported. Resizing keeps the faceplate's aspect
  ratio and snaps to the zoom steps `ZOOM_STEPS` (0.75× to 2×). The zoom is
  saved with the state. CLAP sizes are physical pixels except on macOS,
  where hosts work in points. `set_scale` is honoured elsewhere.
- **Timer.** While the editor is open the shell registers a 16 ms host
  timer, if the host offers the timer extension. Each tick folds host
  changes into the model, ticks and renders the editor, and requests a
  flush if needed.
- **Latency.** Reports `LATENCY`. It is a constant because a latency that
  changed with a parameter would require the host to restart the plugin.

In a DAW the editor window takes keys only while a panel wants typed text.
Otherwise keys go on to the DAW. Names and codes are typed in a platform
text prompt (macOS, Windows) because some DAWs take keys such as Space
first. See [input.md](input.md).

### VST3

VST3 builds come from wrapping the plugin with free-audio's clap-wrapper:
`cargo silkscreen bundle <package> --vst3` builds a self-contained `.vst3`
with the plugin linked in ([building-and-shipping.md](building-and-shipping.md)).
silkscreen has no VST3 code of its own.
Design plugins to the subset both formats carry: stable integer parameter
ids, a fixed latency, one main stereo bus, one note port, and state as an
opaque blob. Every extension the shell registers has a VST3 counterpart
the wrapper maps, so a wrapped build behaves like the CLAP one. Avoid
features that only CLAP carries, such as per-voice modulation or note
expressions.

For tests, the `test-host` feature's `silkscreen_clap::test_host::load::<P>()`
loads a plugin in process through clack-host, as a DAW would
(`examples/gain-plugin/tests`).

## Web: `silkscreen-host-web`

In a browser the editor and the engine are two wasm modules:

- **The editor module** (main thread, wasm-bindgen) holds the editor,
  model, controller and compositor, unchanged from native.
  `silkscreen_host_web::mount(canvas, editor)` returns a `WebHost`.
- **The engine module** runs in an `AudioWorklet`. It is raw C-ABI wasm
  (no wasm-bindgen) made by
  `silkscreen_bridge::export_web_engine!(Engine, constructor)`, and it
  renders 128-frame quanta.

They are joined by a `SharedArrayBuffer`: the UI ring, value slots, telemetry
and a MIDI ring, in a versioned layout (`silkscreen_bridge::sab`) whose
magic and version let modules from different builds refuse each other. The
ring logic is the same Rust at both ends. See
[engine-bridge.md](engine-bridge.md).

### Presentation and input

The editor renders into a frame in wasm memory. Once per
`requestAnimationFrame` the host ticks it and puts the damage rects on a
`<canvas>` with `putImageData`, through a zero-copy `ImageData` view that is
rebuilt when the frame is reallocated or memory grows. The canvas backing
store is CSS size × `devicePixelRatio`. The ratio is read every frame, so
browser zoom and moving between displays reach `set_scale`.
`WebHost::set_zoom` renders the faceplate at a zoom on top of that, sharp
rather than stretched.

Input comes from DOM listeners on the canvas: pointer events with pointer
capture, wheel (normalised to lines), keys while the canvas has focus, and
dropped files (`DropPayload::Files`, resolved with `take_dropped_file`).
Touches and the wheel go to the editor only over a control that takes
input, or anywhere while a panel is open. Elsewhere they scroll and pinch
the page.

### Cross-origin isolation

A `SharedArrayBuffer` exists only on a cross-origin-isolated page, which
means it must be served with:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`silkscreen_host_web::connect(initial, frame)`, with `frame` the
plugin's `PluginUi::frame`, returns the editor's engine link and a
`Connection` (the buffer for the worklet and the MIDI ring's writer). If the
page isn't isolated (`cross_origin_isolated()` is false), the link is a
`NullLink` and the editor runs without sound. `cargo silkscreen web --serve`
and the `_headers` file it writes provide the headers (see
[building-and-shipping.md](building-and-shipping.md)).

### Faceplate PNGs: fetch mode

Natively the faceplate's PNGs are embedded at every scale. On the web, a
faceplate crate built with its `fetch` feature names each PNG by URL
(`assets/<crate>/<file>`) and the page fetches only those the current scale
needs. Turn the feature on for wasm32 only, in the web crate:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
gain = { workspace = true, features = ["fetch"] }
```

Cargo ignores features on a target-specific dependency when building for
other targets, so native builds keep their embedded PNGs. While a PNG is in
flight the best one at hand stands in (scaled down, or scaled up), and the
editor repaints in full when it arrives. A skin's frames and a page group's
pages are requested together. See [faceplates.md](faceplates.md).

### The JavaScript modules

The host's JS lives in `crates/silkscreen-host-web/js`. The constant
`silkscreen_host_web::JS_DIR` gives its absolute path on the machine that
built the crate. The web build copies it beside the page, leaving out the
`*.test.mjs` node tests.

| Module | Role |
|---|---|
| `silkscreen-worklet.js` | The `AudioWorkletProcessor`: instantiates the engine module, supplies its `silkscreen_sab` imports (Atomics over the buffer), copies audio, allocates nothing per quantum, posts the engine's load. |
| `silkscreen-audio.mjs` | `startAudio({ sab, engineUrl, workletUrl, source, channels, onLoad, suspended })`. With `suspended: true` the whole graph loads up front, and the result's `resume()` starts it from a user gesture. `setHostParam(index, value)` stands in for host automation. |
| `silkscreen-keyboard.mjs` | An on-screen piano strip that sends MIDI, plus computer keys (A to ' for white keys, Z and X for octaves). |
| `silkscreen-midi.mjs` | Web MIDI: forwards every channel message from every input. A missing API or refused permission resolves quietly. |
| `silkscreen-fit.mjs` | Fits the faceplate to the window by setting the host's zoom. |
| `silkscreen-load.mjs` | The engine-load meter the worklet reports. |
| `silkscreen-page.mjs` | `mountSynthPage`, a complete instrument page. |
| `silkscreen-survey.mjs` | An optional survey prompt, used by `mountSynthPage`. |

Notes reach the engine as MIDI 1.0 channel messages in the buffer's MIDI
ring. They are applied at the start of each quantum, after the UI's values.

### A page

The smallest page (`examples/gain-web`) exports a `start(canvas)` from its
editor crate that calls `connect` and `mount`, and an `index.html` that calls
`startAudio` from a button. For an instrument with presets,
`silkscreen_host_web::export_page!(TonePage, tone::UI)` exports a
wasm-bindgen class over a `PresetPage`. That class keeps user presets in
IndexedDB under `PluginUi::web_prefix`, restores the session and share
links, and exposes `sab`, `midi`, `editorWantsKeys` and `setZoom` to the
page. `mountSynthPage` then builds the whole page around it: the faceplate
over a keyboard bar pinned to the bottom, a splash whose key press starts
audio and Web MIDI, a CPU meter and MIDI readout, and the fit.

```js
import * as wasm from "./pkg/tone_web.js";
import { mountSynthPage } from "./silkscreen-page.mjs";

mountSynthPage({
  wasm, name: "Tone", blurb: "a one-oscillator synth",
  engine: "./tone_web_engine.wasm", example: "tone-web",
  vendor: { name: "Example Audio", url: "https://example.com" },
  homepage: "https://example.com/tone",
});
```

`wasm`, `name`, `blurb` and `engine` are required. The optional ones are
`example` (the package named in the "not isolated" message), `vendor`
(`{ name, url? }`, shown as "by NAME"), `homepage` (linked as "For more
details, see …"), `accent` (link colour), `logo` (`{ src, alt, width? }`,
replaces the name), `low`, `octaves` and `keysHigh` (the keyboard), and
`survey`. The page leaves `window.page`, `audio`, `keyboard` and `fit` on
the console for debugging.

## Standalone: `silkscreen-host-native` and `silkscreen-cpal`

A standalone app puts the editor in a top-level window and the engine on the
audio device, joined by the native bridge:

```rust
let initial: Vec<f32> = gain::PARAMS.iter().map(|p| p.default).collect();
let (ui, end) = silkscreen::bridge::bridge(&initial, silkscreen::bridge::RING_CAPACITY);
let audio = silkscreen_cpal::start(GainEngine::new, end, silkscreen_cpal::Source::TestTone);
let options = WindowOptions::new("Gain", gain::faceplate::SIZE);
run_standalone(options, move || editor(Box::new(ui)))?;
```

`silkscreen_host_native::run_standalone` opens a baseview window presented
with softbuffer and runs until the window closes. The window resizes between
the smallest and largest zoom steps and snaps to a step. With
`WindowOptions::zoom_keys`, keys 1 to 5 pick one. The same window code,
embedded, is the CLAP editor. On macOS and Windows the native host reads the
system clipboard for Paste and Cmd/Ctrl-V. Other platforms have no
clipboard yet. `DirStore` keeps user presets in the platform's preset
folder.

`silkscreen_cpal::start(make, end, source)` opens the default output device,
builds the engine at the device's rate, and renders it in the output
callback through the bridge's `EngineEnd`. The callback doesn't allocate.
`Source` picks the engine's input: `Silence` (instruments), `TestTone` (a
quiet sawtooth for trying an effect), or `Input`, the default input device,
read through a ring the output callback drains. The input and output must
run at the same rate. `Audio` stops the streams when dropped, and
`Audio::failed` reports an engine panic, after which audio stays silent.
Standalone audio has no MIDI input. If no device opens, the editor still
runs without sound, as `examples/gain-standalone` shows.
