# Building and shipping

Plugins are ordinary cargo packages. One tool, `silkscreen-tool`, does the
steps cargo alone doesn't: wrapping a `cdylib` as a CLAP bundle (and a
VST3, through free-audio's clap-wrapper), signing
and notarizing it on macOS, building and serving a web page from two wasm
modules, and publishing that page to a static site. It works in any
workspace and reads what to build from each package's own `Cargo.toml`.

## The tool

The `silkscreen-tool` package builds a binary called `cargo-silkscreen`, so
once it is installed cargo runs it as a subcommand:

```sh
cargo install --locked --git <silkscreen repository> silkscreen-tool

cargo silkscreen bundle <package> [--debug] [--universal | --target <triple>] [--vst3 [--validate]] [--sign] [--notarize] [--install]
cargo silkscreen web <package> [--debug] [--serve] [--port <n>]
cargo silkscreen deploy-web <package> [--site <checkout>]
```

Inside the silkscreen repository itself, `.cargo/config.toml` defines an
alias that runs the tool from the checkout without installing it:

```toml
[alias]
silkscreen = "run --quiet --package silkscreen-tool --bin cargo-silkscreen --"
```

The tool runs `cargo metadata` in the current directory, so `<package>` is
a member of the current workspace. It builds into cargo's own target
directory (`CARGO_TARGET_DIR` or `target/`). Errors name the package, the
missing key and the fix. An unknown package name lists the members that do
have the section the command needs.

## Package metadata

A plugin package (a `cdylib` that calls `silkscreen_clap::export_clap!`)
declares how to bundle it:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[package.metadata.silkscreen.clap]
bundle = "MyGain"            # the bundle's file name (MyGain.clap) and executable name
id = "com.example.my-gain"   # the macOS bundle id: the plugin's SilkscreenPlugin::ID
name = "My Gain"             # the display name: the plugin's SilkscreenPlugin::NAME
```

A web package (the wasm-bindgen editor crate) declares its engine and page:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[package.metadata.silkscreen.web]
engine = "my-gain-web-engine"   # the engine package, a C-ABI cdylib in the same workspace
page = "web"                    # the page's folder, relative to this Cargo.toml
site = { title = "My Gain", subpath = "products/my-gain/web" }   # optional, for deploy-web
```

All values are non-empty strings. `site` needs both `title` and `subpath`.
Slashes at either end of `subpath` are dropped. The bundle's version comes
from the package's own `version`. `examples/gain-plugin/Cargo.toml` and
`examples/gain-web/Cargo.toml` are working examples.

## CLAP bundles

`cargo silkscreen bundle <package>` builds the package's library (release
unless `--debug`) and writes `<target>/bundled/<bundle>.clap`:

- **macOS:** a bundle directory with `Contents/MacOS/<bundle>`, an
  `Info.plist` (executable, bundle id, name, version, minimum macOS 11.0)
  and `PkgInfo`. `--universal` builds `aarch64-apple-darwin` and
  `x86_64-apple-darwin` and joins them with `lipo`. Both targets must be
  installed with `rustup target add`.
- **Windows and Linux:** the shared library itself, renamed to `.clap`.
- **Windows on Arm:** build with
  `--target arm64ec-pc-windows-msvc`, not `aarch64-pc-windows-msvc`. DAWs
  on Windows on Arm, such as REAPER's Arm build, run as ARM64EC processes:
  they load x64 and ARM64EC plugins but reject plain ARM64 ones at scan.
  ARM64EC code runs natively, NEON included.

`--target <triple>` builds for any other triple. `--universal` and
`--target` can't be combined, and `--universal` runs only on macOS.

`--install` copies the finished bundle, signed if signing was requested, to
the user's CLAP folder, replacing any older copy:

| Platform | Folder |
|---|---|
| macOS | `~/Library/Audio/Plug-Ins/CLAP` |
| Windows | `%LOCALAPPDATA%\Programs\Common\CLAP` |
| Linux | `~/.clap` |

Some hosts hold a loaded plugin's code. Restart the host after installing
over a bundle it has open.

### VST3

`--vst3` also builds `<target>/bundled/<bundle>.vst3`, from free-audio's
clap-wrapper ([hosts.md](hosts.md)):

1. The plugin package's library is built again as a `staticlib` (for both
   architectures with `--universal`, joined by `lipo`), and rustc is asked
   which native libraries it needs (`--print native-static-libs`).
2. clap-wrapper is cloned at the tag the tool pins
   (`silkscreen_tool::vst3::CLAP_WRAPPER_TAG`, v0.18.0). It fetches the
   VST3 SDK and the CLAP headers at the versions that tag pins: VST3 SDK
   3.8.0 and CLAP 1.2.10, both MIT-licensed.
3. A small generated CMake project builds clap-wrapper's VST3 module
   with the static library linked in, and with
   `STATICALLY_LINKED_CLAP_ENTRY`, so the wrapper takes the plugin's
   `clap_entry` from the module itself. The `.vst3` is self-contained: it
   doesn't need the `.clap` installed. Its VST3 class id comes from the
   CLAP id, and its bundle id is the CLAP id with `.vst3` appended.

The output is a bundle on every platform: `Contents/MacOS/<bundle>` on
macOS, `Contents/x86_64-win/<bundle>.vst3` on Windows (the folder layout
Steinberg's validator expects), `Contents/x86_64-linux/<bundle>.so` on
Linux. `--sign` and `--notarize` sign and notarize it as they do the
`.clap`, and `--install` copies it to the user's VST3 folder:

| Platform | Folder |
|---|---|
| macOS | `~/Library/Audio/Plug-Ins/VST3` |
| Windows | `%LOCALAPPDATA%\Programs\Common\VST3` |
| Linux | `~/.vst3` |

clap-wrapper, the SDK and the CMake build are kept under
`<target>/silkscreen-vst3/`, so later builds reuse them. It needs CMake
3.21 or later, git and a C++ compiler (on macOS the Command Line Tools
are enough); a missing tool fails before anything is built, naming what to
install.

`--validate` builds Steinberg's validator from the same SDK and runs it on
the `.vst3`; the bundle command fails if it does. On a Mac with only the
Command Line Tools, the SDK's CMake is told an Xcode version, which it
otherwise insists on.

### Portable builds

To build the same plugin on all three platforms:

- Keep macOS-only code behind `cfg(target_os = "macos")`.
- Ship the faceplate's fonts with the source and pass them to
  `Faceplate::font_dir`, rather than enabling `system_fonts`. A system font
  is missing on one platform and substituted on another, so the baked
  lettering would differ (see [faceplates.md](faceplates.md)).
- PNG snapshots render identically on ARM64 and x86-64, so they can run
  everywhere. Tests that compare audio bit for bit should run only on the
  platform they were recorded on, because libm rounds differently
  elsewhere. Give them tolerance-based siblings that run everywhere.
- Check files compared byte for byte out with LF line endings on every
  platform (`.gitattributes`).
- Linux builds need the ALSA, X11/xcb and Wayland development headers.

## Signing and notarization (macOS)

macOS Gatekeeper rejects an unsigned or un-notarized plugin downloaded from
the web. `--sign` and `--notarize` handle both, and run only on macOS.
Credentials are checked before the build starts, so a missing one fails
fast, and the error names the variables.

`--sign` signs the bundle with `codesign --force --options runtime
--timestamp` (hardened runtime, secure timestamp) using the identity in
`SILKSCREEN_SIGN_IDENTITY`, for example
`"Developer ID Application: <name> (<team id>)"`. It then verifies the
signature strictly. `security find-identity -v -p codesigning` lists the
identities on the machine. The certificate needed is a Developer ID
Application certificate. No entitlements are added: the plugin runs in the
host's process and asks for nothing of its own.

`--notarize` implies `--sign`. It zips the bundle with `ditto`, submits it
with `xcrun notarytool submit --wait`, prints the notary log if the
submission isn't accepted, staples the ticket to the bundle, and has
`spctl` assess the result. Notary credentials come from either:

| Variables | Use |
|---|---|
| `SILKSCREEN_NOTARY_PROFILE` | A keychain profile saved by `xcrun notarytool store-credentials`. Suits a development machine. |
| `SILKSCREEN_NOTARY_KEY`, `SILKSCREEN_NOTARY_KEY_ID`, `SILKSCREEN_NOTARY_ISSUER` | An App Store Connect API key: the `.p8` file's path, its key id and the issuer id. Suits CI: no two-factor prompt, and it can be revoked on its own. All three are required. |

If both are set, the profile wins.

Without `--sign`, a local bundle keeps the linker's ad-hoc signature, which
is fine for the machine that built it. To distribute a signed bundle, zip it
with `ditto -c -k --keepParent` so the signature and stapled ticket survive.
Archivers that drop file modes break it. The tool doesn't sign Windows
builds.

## Web pages

`cargo silkscreen web <package>` builds a web package and its engine for
`wasm32-unknown-unknown` (release unless `--debug`) and assembles
`<target>/web/<package>/`:

| Path | Contents |
|---|---|
| `pkg/` | The editor module, post-processed by the `wasm-bindgen` CLI (`--target web`). |
| `<engine_lib>.wasm` | The engine module, copied as built. |
| `assets/<crate>/…` | Every PNG a fetch-mode faceplate lists in its `.assets` manifest, at its URL. |
| `silkscreen-*.mjs`, `silkscreen-worklet.js` | `silkscreen-host-web`'s JS, without its `*.test.mjs` node tests. |
| the page folder's files | `index.html` and whatever else the page has. |
| `_headers` | The COOP and COEP headers for static hosts that read one. |

The tool finds `silkscreen-host-web`'s JS beside that crate's manifest,
wherever cargo fetched it from (this workspace, a git checkout or a
registry), and the fetched PNGs through the build scripts' `OUT_DIR`s in
cargo's JSON messages. See [hosts.md](hosts.md) for the modules and
[faceplates.md](faceplates.md) for fetch mode.

`--serve` then serves the folder at `http://localhost:8080/` (or
`--port <n>`) with the cross-origin isolation headers a
`SharedArrayBuffer` needs. The server is a small std-only one built into the
tool, so no node or other server is needed. It binds 127.0.0.1, sends
`Cache-Control: no-store`, and refuses paths that climb out of the folder.
Served without the headers, for example from a plain file server, the
editor runs with no engine.

### The wasm-bindgen pin

The `wasm-bindgen` CLI that post-processes the editor module must be exactly
the version of the `wasm-bindgen` crate in `Cargo.lock`. The workspace pins
the crate exactly (`wasm-bindgen = "=0.2.129"`). Before building, the tool
compares the two and, if the CLI is missing or differs, fails with the
install command:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

### wasm SIMD

The silkscreen workspace builds wasm32 with SIMD, in `.cargo/config.toml`:

```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "target-feature=+simd128"]
```

A plugin workspace should set the same for its engine. Every browser that
can run the page (Chrome 91+, Firefox 89+) supports it. A browser without
SIMD refuses the module at compile time rather than running it slowly. The
same file sets `wasm-bindgen-test-runner` as the wasm32 test runner.

## Deploying a page

`cargo silkscreen deploy-web <package>` publishes a page into the git
checkout of a static site whose host deploys on push and reads a root
`_headers` file (Cloudflare Pages, for one). It:

1. builds the page in release, as `cargo silkscreen web` does;
2. mirrors it into `static/<subpath>/` with `rsync --delete`, leaving out
   the page's own `_headers` (such hosts read only the root one) and any
   node tests;
3. appends a block for `/<subpath>/*` to `static/_headers` if none is there,
   with `Cross-Origin-Opener-Policy: same-origin`,
   `Cross-Origin-Embedder-Policy: require-corp` and
   `Cross-Origin-Resource-Policy: same-origin`. The file may hold one block
   per page, so it is only appended to, never rewritten;
4. stages those two paths only, since the checkout may hold other work,
   commits them with the silkscreen workspace's short git hash in the
   message, and pushes.

If nothing changed it commits nothing. The page's place is the `site` table
in `[package.metadata.silkscreen.web]`; a package without one is refused.
The environment adjusts the rest:

| Variable | Effect |
|---|---|
| `SITE` | The site checkout, if `--site` isn't given. One of them is required. |
| `SUBPATH` | Overrides `site.subpath`. |
| `BRANCH` | The branch to push (default `main`). |
| `NO_BUILD` | Reuse `<target>/web/<package>/` as already built. |
| `NO_PUSH` | Commit but don't push. |

After it goes live, check the headers with
`curl -sI https://<site>/<subpath>/ | grep -i cross-origin`.
