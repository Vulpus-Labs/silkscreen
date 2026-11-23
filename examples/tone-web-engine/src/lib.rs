//! The tone example's engine module for the web ([design/hosts.md]): raw
//! C-ABI exports for an `AudioWorklet`, taking notes from the SAB's MIDI
//! ring. Built and served by `cargo silkscreen web tone-web`.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

silkscreen_bridge::export_web_engine!(tone::engine::ToneEngine, tone::engine::ToneEngine::new);
