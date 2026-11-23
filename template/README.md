# Getting started: a silkscreen plugin

This folder is a complete plugin to copy and make your own: one small synth
(a single voice, a wave selector and a level knob) built three ways from
the same code.

| Crate | What it is |
|---|---|
| `my-synth` | The parameters, the faceplate (`my-synth.svg`), the editor and the engine. Every host uses it. |
| `my-synth-clap` | The CLAP plugin. |
| `my-synth-web` | The web page: the editor on a canvas. |
| `my-synth-web-engine` | The engine as a raw wasm module for the page's `AudioWorklet`. |
| `my-synth-standalone` | A desktop app in its own window. |

How the pieces fit is in [`design/overview.md`](../design/overview.md);
the other documents in [`design/`](../design/) cover each part.

## 1. Copy it out

1. Copy this folder to where your project will live.
2. In `Cargo.toml`, give each `silkscreen*` dependency
   `git = "https://github.com/Vulpus-Labs/silkscreen", tag = "…"` in place
   of its `path`. Pick a tag and keep every silkscreen crate on the same
   one.
3. Rename `my-synth` throughout: folder and package names, `MySynthPlugin`,
   `MySynthPage`, `MyEngine`, and the `.wasm` and `.js` names in
   `my-synth-web/web/index.html`. The library names are the package names
   with `_` for `-`. The faceplate's file name must agree in three
   places: the file itself (`my-synth.svg`), `build.rs`, which renders
   it, and the `include!` in `src/lib.rs`, which names the generated
   code after it (`my-synth.rs`). The legend printed on the panel is in
   the SVG.
4. Set your identity:
   - `my-synth-clap/src/lib.rs`: `ID` (reverse-DNS, never changed once
     released: hosts save projects under it) and `NAME`;
   - `my-synth-clap/Cargo.toml`: `[package.metadata.silkscreen.clap]`, with
     the same `id` and `name`, and the bundle's file name;
   - `my-synth/src/lib.rs`: `UI`'s `vendor` and `product`, which also name
     the folder the user's presets are saved in, and `web_prefix`.

## 2. Install the tools

- **Rust.** `rust-toolchain.toml` pins the compiler, so `rustup` installs
  it, with the `wasm32-unknown-unknown` target, on the first build. The pin
  keeps PNG snapshots reproducible: rasterisation can shift by a pixel
  between compiler versions. Bump it on purpose, and re-bless snapshots in
  the same change.
- **silkscreen's build tool**, which bundles plugins and builds pages:

  ```sh
  cargo install --locked --git https://github.com/Vulpus-Labs/silkscreen --tag … silkscreen-tool
  ```

  It is then `cargo silkscreen …`. It reads what to build from
  `[package.metadata.silkscreen]` in each package's `Cargo.toml`.
- **The wasm-bindgen CLI**, for the web page. It must be exactly the
  version of the `wasm-bindgen` crate silkscreen pins, 0.2.129:

  ```sh
  cargo install wasm-bindgen-cli --version 0.2.129 --locked
  ```

  `cargo silkscreen web` checks the version against your `Cargo.lock` and
  names the install command if they differ.

## 3. Build and run

```sh
cargo test                                          # engine, editor and snapshot tests
cargo silkscreen bundle my-synth-clap --install     # the .clap, into your CLAP folder
cargo silkscreen web my-synth-web --serve           # the page, at http://localhost:8080
cargo run -p my-synth-standalone                    # the desktop app
```

- **The plugin.** The bundle lands in `target/bundled/MySynth.clap`, and
  `--install` copies it to the user's CLAP folder. Restart the DAW or have
  it rescan. `--universal` builds one macOS bundle for both Apple silicon
  and Intel; `--sign` and `--notarize` sign it for distribution (see
  [`design/building-and-shipping.md`](../design/building-and-shipping.md)).
- **The page.** The engine and editor share a `SharedArrayBuffer`, which
  browsers allow only on a cross-origin isolated page. `--serve` sends the
  headers that make it so; any host you publish to must send them too (a
  `_headers` file in the build has them). Chrome, Edge and Firefox work;
  Safari doesn't. The page is built in `target/web/my-synth-web/`.
- **The desktop app.** It shows and edits the faceplate, with the engine on
  the default audio device. It takes no notes yet: play the synth in a DAW
  or on the page.

## 4. Change the faceplate

`my-synth.svg` is the whole visual design. Everything outside the layer
`<g id="controls">` is rendered to PNG at build time, at each scale the
editor can show. Each shape inside that layer marks one control: its id is
`kind.name` (`knob.level`), and its bounds are the control's. The build
generates a `Controls` struct with a field per control, which
`src/lib.rs` fills with a binding to a parameter.

Text in the SVG needs its font: `build.rs` loads Jost, the face the
editor's own text uses (family name `Jost*`), with
`Faceplate::font_dir(silkscreen_build::TEXT_FONT_DIR)`. Load your own
fonts the same way, or convert text to paths. A build error names the
element at fault and the fix. The details, including skins, canvas
controls and pages, are in [`design/faceplates.md`](../design/faceplates.md).

## 5. Snapshots

`my-synth/tests/editor.rs` drives the editor headlessly and compares what
it draws with the PNGs in `my-synth/tests/snapshots/`. A test whose
snapshot doesn't exist yet fails and says so. When a change is meant to
alter what the editor draws:

```sh
SILKSCREEN_BLESS=1 cargo test     # rewrite the snapshots
git diff --stat                   # then look at every changed PNG
```

A mismatch leaves `name.actual.png` and `name.diff.png` beside the
snapshot, to see what moved. Snapshots are bit-exact on the platform they
were blessed on; bless them on one machine.

## 6. The engine

`MyEngine` implements `silkscreen::bridge::Engine`. Hosts call it on the
audio thread, or in an `AudioWorklet` on the web, so it must not allocate,
lock or block: build everything in `new`, which hosts call off the audio
thread. Parameter changes arrive through `set_param`, notes through
`note`, and `process` fills the output. The bridge between editor and
engine is described in [`design/engine-bridge.md`](../design/engine-bridge.md).
