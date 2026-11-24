# Changelog

Changes to silkscreen's crates, which share one version and are released
together as a git tag, `vX.Y.Z`. Before 1.0, a breaking change raises the
minor version (0.2 to 0.3) and anything else the patch version.

The public API is everything the crates export, except items marked
`#[doc(hidden)]` (such as `__private` modules), which exist for macros and
tests and may change in any release. CI runs `cargo semver-checks` against
the latest tag, so a breaking change fails until the version is raised.

## Unreleased

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
