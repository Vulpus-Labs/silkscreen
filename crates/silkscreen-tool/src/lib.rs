//! Builds silkscreen plugins from any workspace.
//!
//! ```text
//! cargo silkscreen bundle <package> [--debug] [--universal | --target <triple>] [--vst3 [--validate]] [--sign] [--notarize] [--install]
//! cargo silkscreen web <package> [--debug] [--serve] [--port <n>]
//! cargo silkscreen deploy-web <package> [--site <checkout>]
//! ```
//!
//! What to build is read from the packages' own manifests, through
//! `cargo metadata`:
//!
//! ```toml
//! # A plugin package (a `cdylib` with `silkscreen_clap::export_clap!`).
//! [package.metadata.silkscreen.clap]
//! bundle = "MyGain"               # the bundle's file name and executable
//! id = "com.example.my-gain"      # its bundle id: the plugin's `ID`
//! name = "My Gain"                # its display name: the plugin's `NAME`
//!
//! # A web package (the wasm-bindgen editor).
//! [package.metadata.silkscreen.web]
//! engine = "my-gain-web-engine"   # the engine package, built beside it
//! page = "web"                    # the page's folder, from this manifest
//! site = { title = "My Gain", subpath = "products/my-gain/web" }  # for deploy-web
//! ```
//!
//! [`bundle`] builds a `.clap` into `<target>/bundled/`: a bundle
//! directory on macOS (`--universal` joins ARM64 and x86-64 with `lipo`;
//! [`sign`] signs and notarizes it), the shared library elsewhere; with
//! `--vst3`, [`vst3`] builds a `.vst3` beside it through clap-wrapper.
//! [`web`] builds the two wasm modules, the host's JS and the page into
//! `<target>/web/<package>/`, and serves them with the cross-origin
//! isolation a `SharedArrayBuffer` needs. [`deploy`] publishes a page into
//! a static site's git checkout.

pub mod bundle;
pub mod deploy;
mod files;
pub mod metadata;
pub mod sign;
pub mod vst3;
pub mod web;

pub use metadata::{Clap, Package, Site, Web, Workspace};

/// The commands and their flags.
pub fn usage() -> String {
    "usage: cargo silkscreen bundle <package> [--debug] [--universal | --target <triple>] [--vst3 [--validate]] [--sign] [--notarize] [--install]\n       \
     cargo silkscreen web <package> [--debug] [--serve] [--port <n>]\n       \
     cargo silkscreen deploy-web <package> [--site <checkout>]   (env: SITE, SUBPATH, BRANCH, NO_BUILD, NO_PUSH)"
        .into()
}

/// Runs a command line (without the program's name), in the workspace of
/// the current directory.
pub fn run(args: &[String]) -> Result<(), String> {
    let [cmd, package, flags @ ..] = args else {
        return Err(usage());
    };
    let ws = Workspace::load(None)?;
    match cmd.as_str() {
        "bundle" => bundle::run(&ws, package, flags),
        "web" => web::run(&ws, package, flags),
        "deploy-web" => deploy::run(&ws, package, flags),
        _ => Err(usage()),
    }
}

/// `cargo`, or the one that is running us.
fn cargo() -> std::process::Command {
    std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

/// Run `cmd`, failing with its name and status.
fn run_command(cmd: &mut std::process::Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("running {cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed: {status}"))
    }
}
