//! Native window host for silkscreen editors: baseview for the window,
//! softbuffer to present the CPU-rendered frame ([design/rendering.md]).
//!
//! [`run_standalone`] opens a top-level window; [`open_embedded`] puts the
//! same handler inside a host's window (a CLAP editor).
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md

mod clipboard;
mod menu;
mod present;
mod preset_io;
mod presets;
mod prompt;
mod translate;
mod window;
mod zoom;

pub use presets::DirStore;
pub use window::{
    EmbeddedWindow, ParentWindow, SharedState, WindowOptions, open_embedded, run_standalone,
};
pub use zoom::{ZOOM_STEPS, nearest_zoom, zoom_for_size};
