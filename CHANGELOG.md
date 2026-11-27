# Changelog

Changes to silkscreen's crates, which share one version and are released
together as a git tag, `vX.Y.Z`. Before 1.0, a breaking change raises the
minor version (0.2 to 0.3) and anything else the patch version.

The public API is everything the crates export, except items marked
`#[doc(hidden)]` (such as `__private` modules), which exist for macros and
tests and may change in any release. CI runs `cargo semver-checks` against
the latest tag, so a breaking change fails until the version is raised.

## Unreleased

## 0.3.2 (2026-11-27)

### Added

- The web host supplies a transport (design/engine-bridge.md): `WebEngine`
  gives its engine `Engine::transport` every block, stopped at beat 0 in
  4/4, with a tempo the page sets through `WebEngine::set_tempo`, the
  engine module's new `silkscreen_engine_set_tempo` export, and
  `startAudio`'s `setTempo(bpm)`. Engines whose `transport` hook was never
  called on the web now are, every block. The worklet skips the call for
  engine modules built before this export.
- `mountSynthPage` options (design/hosts.md): `tempo`, a BPM field
  remembered between visits; `links`, a bar of links in the top right;
  and `signup`, a mailing-list form opened from that bar, posted in a new
  tab.

## 0.3.1 (2026-11-25)

### Fixed

- A CLAP plugin's engine frame (`Engine::frame`) now reaches its editor:
  the licence gate that stands between the CLAP processor and every
  engine didn't forward it, so a plugin's frame was never asked for. A
  test now checks that the gate forwards every `Engine` hook.

## 0.3.0 (2026-11-25)

### Added

- Telemetry frames (design/engine-bridge.md): an engine publishes a
  snapshot of many values with `Engine::frame`, once per block, and the
  editor reads the latest complete one with `EditorState::engine_frame`,
  never a torn one. The tap never waits or allocates. A plugin sizes its
  frame with `PluginUi::frame`. Also `bridge_with_frame`,
  `telemetry_with_frame`, `SabLayout::with_frame`,
  `TelemetryTap::write_frame`, `TelemetryReader::poll_frame` and
  `frame`, and `Harness::engine_frame` for tests.

### Changed (breaking)

- `PluginUi` has a `frame` field: add `frame: 0` to a plugin that
  publishes no frame.
- `silkscreen_host_web::connect` and `sab_link` take the frame size:
  pass `PluginUi::frame`.
- `LayoutError` has a `Frame` variant, and the shared buffer's layout is
  version 4: rebuild a web page's UI and engine modules together.

## 0.2.2 (2026-11-24)

### Added

- `cargo silkscreen bundle <package> --vst3` builds a VST3 beside the
  CLAP, from free-audio's clap-wrapper (v0.18.0, which pins VST3 SDK
  3.8.0): the plugin's library linked statically into the wrapper's VST3
  module, so the `.vst3` stands alone. `--sign`, `--notarize` and
  `--install` handle it with the `.clap`; `--validate` builds Steinberg's
  validator and runs it. Needs CMake, git and a C++ compiler.

## 0.2.1 (2026-11-24)

### Fixed

- `silkscreen-clap`'s `test-host` feature now builds on its own: it
  enables the `clack-host` feature of `clack-extensions` that
  `test_host` needs, which before only the workspace's examples did.

### Added

- CI checks every change for semver breaks against the latest release tag.

## 0.2.0 (2026-11-23)

First public release. Crates: `silkscreen`, `silkscreen-core`,
`silkscreen-model`, `silkscreen-text`, `silkscreen-bridge`,
`silkscreen-build`, `silkscreen-clap`, `silkscreen-host-web`,
`silkscreen-host-native`, `silkscreen-cpal`, `silkscreen-harness`,
`silkscreen-tool`.

- SVG faceplates rendered to PNG at build time, with generated layout
  code for their controls.
- Controls, bindings, presets, undo, dialogs and overlays, drawn on the
  CPU.
- A lock-free bridge to the audio engine; engines depend only on
  `silkscreen-bridge`, and can be `no_std`.
- Hosts: CLAP plugins, web pages (the engine in an `AudioWorklet`) and a
  standalone desktop app.
- A pluggable licence check (`CheckCode`); silkscreen ships no licensing
  scheme of its own.
- `cargo silkscreen` to bundle, sign and notarize plugins, and to build,
  serve and deploy web pages.
- A headless test harness with PNG snapshots.
- A getting-started template and the `gain`, `tone` and `widgets`
  examples.
