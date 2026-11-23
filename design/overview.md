# silkscreen overview

silkscreen is a Rust library for audio plugin editors. An editor is a
faceplate drawn as SVG, with a modest set of interactive controls laid
over it. silkscreen renders that faceplate and its controls on the CPU,
turns host input into control-level gestures, routes every state change
through a model and controller, and talks to the audio engine over a
lock-free bridge.

To start a plugin of your own, copy the template: [`template/README.md`](../template/README.md)
walks through it.

It is a faceplate library, not a general GUI toolkit. Layouts don't
reflow, there is no general text editing, no theming engine, no
accessibility tree and no retained widget tree. Geometry comes from the
SVG at build time, so a plugin's editor is mostly generated code plus
the bindings that connect its controls to parameters.

## Targets

One editor serves three targets:

1. **CLAP plugin.** The editor is embedded in the DAW's window through
   `silkscreen-clap`, which is built on clack. See [hosts.md](hosts.md).
2. **Web.** The editor is compiled to wasm and draws into a `<canvas>`;
   the engine runs as a separate wasm module in an AudioWorklet, and the
   two talk over a `SharedArrayBuffer`.
3. **Standalone app.** A top-level native window, with audio I/O from
   `silkscreen-cpal`.

A fourth host, the headless test harness (`silkscreen-harness`), drives
the same editor with scripted input and checks PNG snapshots. Every
feature should be exercisable headlessly; one that isn't is a sign the
design needs another look.

## The host contract

Every host drives the same editor through a small contract. A host:

- translates its native input into `RawEvent`s in logical coordinates,
  each with a millisecond timestamp (see [input.md](input.md)),
- calls `tick(now_ms)` periodically,
- reports scale changes with `set_scale`,
- calls `render` into a pixel buffer of `physical_size()` and presents
  the damage rects it returns (see [rendering.md](rendering.md)).

The contract lives in the `silkscreen` crate and has two halves:

- `EditorState` lives as long as the plugin instance: model, controller,
  event-log cursors and the engine link. Engine traffic and host changes
  keep flowing while no window is open.
- `EditorView` lives as long as a window: gesture recognizer,
  compositor, faceplate raster and scale.

`Editor` bundles the two for hosts that own both (web, standalone,
tests). The CLAP host keeps an `EditorState` for the plugin's lifetime
and makes an `EditorView` each time the DAW opens the editor. Every
method is called on the UI thread; the only cross-thread traffic is
inside the engine link.

Changes to this contract touch every host, so it stays small.

## Crates

| Crate | Role |
|---|---|
| `silkscreen-core` | Geometry, `RawEvent`, the gesture recognizer, layout types, `RasterSource`, widgets and the compositor. Depends only on tiny-skia. |
| `silkscreen-text` | Runtime text: an embedded face rasterised with fontdue, cached per glyph and size. |
| `silkscreen-bridge` | The engine side: `EngineEvent` and its 16-byte codec, the `Engine` trait, `EngineLink`, parameter slots, transports and telemetry. |
| `silkscreen-model` | `ParamSpec`, `Model`, the event log and cursors, undo, `Controller` and the `Binding` trait with its stock bindings. |
| `silkscreen` | The facade: `Editor`, `EditorState`, `EditorView`, panels, presets, tooltips, and re-exports of `core`, `model` and `bridge`. |
| `silkscreen-build` | Build time, from `build.rs`: renders the SVG faceplate to PNGs at each scale and generates the layout code. |
| `silkscreen-harness` | The headless host: scripted input on a virtual clock, PNG snapshots. |
| `silkscreen-host-native` | A baseview window, presented through a `CALayer` on macOS and softbuffer elsewhere; parented for CLAP, top-level for standalone. |
| `silkscreen-clap` | The CLAP shell: gui, params, state, timer and note-port extensions, gesture forwarding, the licensing hook. |
| `silkscreen-host-web` | wasm-bindgen and web-sys: canvas presentation, DOM input, the `requestAnimationFrame` loop, the `SharedArrayBuffer` transport, and the page's JavaScript modules. |
| `silkscreen-cpal` | Standalone audio: a cpal output stream driving an `Engine` through the native bridge. |
| `silkscreen-tool` | The `cargo-silkscreen` binary: bundles, signs and notarizes CLAP plugins, builds and serves web pages, and deploys them. See [building-and-shipping.md](building-and-shipping.md). |

Dependencies point one way:

- `silkscreen-core`, `silkscreen-text` and `silkscreen-bridge` are leaves
  and independent of each other. `core` knows nothing of parameters or
  engines.
- `silkscreen-model` depends on `core` and `bridge`.
- `silkscreen` depends on `core`, `model`, `bridge` and `text`.
- `silkscreen-build` depends on `core`, `model` and `text`, and is only
  ever a build dependency.
- The hosts (`silkscreen-harness`, `silkscreen-host-native`,
  `silkscreen-host-web`) depend on `silkscreen`. `silkscreen-clap`
  depends on `silkscreen` and `silkscreen-host-native`.
- `silkscreen-cpal` depends only on `silkscreen-bridge`.
- `silkscreen-tool` depends on none of the library crates; it reads a
  package's configuration through `cargo metadata`.

silkscreen ships no DSP and no licence scheme. A plugin brings its own
DSP, and plugs its own code check into the licensing hook (see
[licensing-hook.md](licensing-hook.md)).

## Examples

- `examples/gain` is the smallest effect and `examples/tone` the
  smallest instrument. Each has an editor crate with its engine, and a
  crate per target: `gain-plugin`, `gain-web`, `gain-web-engine`,
  `tone-plugin`, `tone-web`, `tone-web-engine`, plus `gain-standalone`
  and `gain-preview` (an editor in a window with no audio).
- `examples/widgets` exercises every control kind, panel and overlay as
  snapshot tests.

## Rules for wasm crates

`silkscreen-core`, `silkscreen-model` and `silkscreen-text` must build
for `wasm32-unknown-unknown`. That means:

- no threads,
- no `std::time::Instant`,
- no filesystem.

Time is never read from a clock inside the library. Hosts pass it in as
`now_ms: f64`, through `tick` and through each `RawEvent`'s `time_ms`.
Only differences between times matter, so a host may use any monotonic
origin. This also makes every test deterministic: the harness advances a
virtual clock.

`silkscreen-bridge` builds without its default `std` feature for
`no_std` engines. Without it, only the protocol is available
(`EngineEvent`, the codec, `EngineLink`); the transports and telemetry
need `std`.

## Rules for engine code

- Engine code depends only on `silkscreen-bridge`, either directly or
  through the facade's `silkscreen::bridge` re-export. It never touches
  the model, the editor or any host crate, which is what lets the same
  engine run inside a CLAP plugin, a cpal stream and an AudioWorklet.
- Bridge code on the engine side must not allocate or lock. It runs on
  the audio thread; transports are rings and atomics natively and a
  `SharedArrayBuffer` with `Atomics` on the web. `silkscreen-bridge`'s
  `testing` feature provides an allocation-counting allocator so tests
  can check this.

See [engine-bridge.md](engine-bridge.md) for the protocol and
[model.md](model.md) for the UI side of it.

## Conventions

- **Generated, typed code over runtime lookup.** Control IDs and geometry
  are generated from the SVG at build time, and an unbound control is a
  compile error. See [faceplates.md](faceplates.md).
- **Build-time errors name the element and the fix.** A failure in
  `silkscreen-build` says which SVG element is wrong and what to do about
  it.
- **Everything visual is snapshot-tested.** Every visual change can be
  checked with a headless PNG snapshot, and every interaction by
  replaying an event script. Re-bless snapshots with
  `SILKSCREEN_BLESS=1 cargo test` and inspect the diffs.
- **No WebView.** Text, tooltips and menus are drawn by silkscreen itself
  (see [overlays-and-text.md](overlays-and-text.md)). The web build is a
  port of the same Rust, not a separate JavaScript UI.
