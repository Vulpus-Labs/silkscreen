//! My Synth's page: the editor on a canvas, presets kept in the browser.
//! `cargo silkscreen web my-synth-web --serve` builds it with its engine
//! module and serves it with the headers a `SharedArrayBuffer` needs.

silkscreen_host_web::export_page!(MySynthPage, my_synth::UI);
