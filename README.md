# silkscreen

A Rust library for audio plugin editors. You draw the faceplate as SVG;
silkscreen renders it to PNG at build time, lays a small set of
interactive controls over it, and draws them on the CPU. All state flows
through a model and controller to the audio engine over a lock-free
bridge. The same editor runs as a CLAP plugin, on a web page (wasm, with
the engine in an `AudioWorklet`), and as a standalone desktop app.

It is a faceplate library, not a general GUI toolkit: layouts come from
the SVG, and a plugin's editor is mostly generated code plus the bindings
from its controls to its parameters.

**Status:** pre-1.0. The API changes between minor versions; pin a tag.
Changes are listed in [`CHANGELOG.md`](CHANGELOG.md).

## Start here

- [`template/`](template/): a complete small synth to copy, with a
  getting-started guide in [`template/README.md`](template/README.md).
- [`design/`](design/): how silkscreen works, one document per area,
  starting with [`design/overview.md`](design/overview.md).
- [`examples/`](examples/): `gain` (one knob, as a plugin, page and app),
  `tone` (an instrument with note input), `widgets` (every control kind,
  presets, overlays and dialogs, under snapshot tests).

## Crates

| Crate | What it does |
|---|---|
| `silkscreen` | The editor: controls, bindings, presets, dialogs, the host contract. What a plugin's editor depends on. |
| `silkscreen-core` | Geometry, layout, input, gestures, rasters and the compositor. |
| `silkscreen-model` | Parameters, the model, bindings and undo. |
| `silkscreen-text` | Runtime text, with Jost Medium embedded. |
| `silkscreen-bridge` | The engine side: the `Engine` trait, the lock-free link, telemetry. Engines depend only on this. |
| `silkscreen-build` | Build scripts: SVG faceplates to PNGs and generated layout code, factory preset banks, LCD fonts. |
| `silkscreen-clap` | The CLAP plugin host. |
| `silkscreen-host-web` | The web host and its JavaScript. |
| `silkscreen-host-native` | Native windows: the plugin's editor window and the standalone app. |
| `silkscreen-cpal` | Audio I/O for the standalone app. |
| `silkscreen-harness` | A headless host for tests: scripted input and PNG snapshots. |
| `silkscreen-tool` | `cargo silkscreen`: bundles plugins (signed and notarized on macOS), builds and serves web pages, deploys them. |

Use them from git, all at one tag:

```toml
silkscreen = { git = "https://github.com/Vulpus-Labs/silkscreen", tag = "…" }
```

## Working on silkscreen

The toolchain is pinned in `rust-toolchain.toml` so PNG snapshots are
reproducible. The checks CI runs are listed in [`CLAUDE.md`](CLAUDE.md).
After a deliberate visual change, re-bless snapshots with
`SILKSCREEN_BLESS=1 cargo test` and inspect the diffs before committing.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT licence](LICENSE-MIT), at your option. The bundled Jost font is under
the SIL Open Font License 1.1
([`crates/silkscreen-text/fonts/OFL.txt`](crates/silkscreen-text/fonts/OFL.txt));
VT323, used by the web host, likewise
([`crates/silkscreen-host-web/js/fonts`](crates/silkscreen-host-web/js/fonts)).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
licence, shall be dual licensed as above, without any additional terms
or conditions.
