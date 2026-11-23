//! Open several installed CLAP bundles' editors in one process and one
//! parent window, as a DAW does, and report each gui call. Catches
//! cross-plugin collisions: two plugins that each register an
//! Objective-C class of the same name, say.
//!
//! `cargo silkscreen bundle gain-plugin --install && cargo silkscreen bundle tone-plugin --install`
//! then `cargo run -p tone-plugin --example gui_smoke [Bundle ...]`
//! (defaults to `SilkscreenGain SilkscreenTone`). Add `--cycles 50` to
//! open and close the editors 50 times and check memory stays flat.
//! macOS only.

#[cfg(target_os = "macos")]
mod smoke;

#[cfg(target_os = "macos")]
fn main() {
    smoke::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("gui_smoke runs on macOS only");
}
