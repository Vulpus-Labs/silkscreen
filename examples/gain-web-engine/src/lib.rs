//! The gain example's engine module for the web ([design/hosts.md]): raw C-ABI
//! exports (no wasm-bindgen) that an `AudioWorklet` instantiates. Built and
//! served by `cargo silkscreen web gain-web`.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

silkscreen_bridge::export_web_engine!(gain::engine::GainEngine, gain::engine::GainEngine::new);
