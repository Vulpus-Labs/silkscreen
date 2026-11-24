# silkscreen: working conventions

## Process

- `design/` describes how silkscreen works now, one document per area. A
  change that alters how something works updates its document in the same
  change. Code comments cite the document (`design/presets.md`).
- Work is tracked in `tickets/` and `epics/` (see `tickets/README.md`).
  When a ticket's acceptance criteria are met, tick them, add a
  `## Close-out (YYYY-MM-DD)` section and `git mv` it to `tickets/closed/`.
- Keep commits to one change each, with a subject that says what it does.
  A commit for a ticket names it in the subject (`0007: ...`).
- Record user-visible changes under `## Unreleased` in `CHANGELOG.md`. A
  breaking change raises the version (0.x: the minor) in the same change;
  CI's semver check against the latest tag enforces it.
- A release: move Unreleased under the new version and date, set
  the version in each crate's manifest, `[workspace.package]` and
  `[workspace.dependencies]`, tag `vX.Y.Z` and push the tag with the
  commit.

## Code

- `silkscreen-core`, `silkscreen-model` and `silkscreen-text` must build for
  `wasm32-unknown-unknown`: no threads, no `std::time::Instant`, no
  filesystem. Time is passed in by hosts as `now_ms: f64`.
- `silkscreen-core` knows nothing of params or engines. Engine code depends
  only on `silkscreen-bridge` (directly, or through `silkscreen::bridge`).
- Engine-side bridge code must not allocate or lock.
- Prefer generated, typed code over runtime lookup. Build-time errors must
  name the offending element and the fix.
- `template/` must keep building against the crates beside it.

## Verification

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p silkscreen-core -p silkscreen-model -p silkscreen-text --target wasm32-unknown-unknown
cargo check -p silkscreen-bridge --no-default-features   # no_std engines
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo package -p silkscreen-core -p silkscreen-text -p silkscreen-model -p silkscreen-build -p silkscreen-bridge -p silkscreen -p silkscreen-harness -p silkscreen-host-native -p silkscreen-host-web -p silkscreen-cpal -p silkscreen-tool
(cd template && cargo fmt --all --check && cargo test && cargo clippy --all-targets -- -D warnings)
cargo semver-checks --workspace --baseline-rev "$(git describe --tags --abbrev=0)"
```

Visual changes are checked by PNG snapshots. Re-bless with
`SILKSCREEN_BLESS=1 cargo test` and inspect the diffs before committing.
