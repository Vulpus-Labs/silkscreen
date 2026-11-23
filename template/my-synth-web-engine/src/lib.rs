//! My Synth's engine for the web: C-ABI exports, no wasm-bindgen, that the
//! page's `AudioWorklet` instantiates. Built by `cargo silkscreen web`.

silkscreen::bridge::export_web_engine!(my_synth::engine::MyEngine, my_synth::engine::MyEngine::new);
