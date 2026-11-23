//! Platform-independent UI primitives for silkscreen.
//!
//! Geometry, host-neutral input ([design/input.md]), layout types generated from the
//! SVG faceplate ([design/faceplates.md]), raster sources ([design/rendering.md]) and the CPU
//! compositor ([design/rendering.md]). This crate knows nothing of parameters, engines
//! or platforms, and must stay buildable for `wasm32-unknown-unknown`: no
//! threads, no `std::time::Instant`, no filesystem.
//!
//! [design/rendering.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/rendering.md
//! [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
//! [design/input.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/input.md

pub mod assets;
pub mod compositor;
pub mod geom;
pub mod gesture;
pub mod input;
pub mod layout;
pub mod lcd;
pub mod raster;
pub mod telemetry;
pub mod text;
pub mod widget;

pub use compositor::{Compositor, Overlay};
pub use geom::{IRect, Point, Rect, Size, Vec2};
pub use gesture::{ControlEvent, GestureConfig, GestureOutput, GestureRecognizer};
pub use input::{DropPayload, Key, Modifiers, MouseButton, RawEvent, RawKind};
pub use layout::{ControlId, ControlKind, ControlSpec, Layout, MouseCursor, PageGroup, PageRef};
pub use lcd::{LcdBitmap, LcdFont, LcdStyle};
pub use raster::{
    EmbeddedPngs, EmbeddedRaster, RasterSource, SharedSkin, Skin, SkinCache, SkinRaster,
};
pub use telemetry::{
    Ballistics, Meter, MeterReading, MeterStyle, SCOPE_WINDOW, Scope, ScopeStyle, ScopeTrace,
};
pub use tiny_skia;
pub use widget::{
    Bat, Blank, Fader, FaderGeometry, Field, Knob, Lamp, Push, Selector, SpriteFader, SpriteKnob,
    SpriteSwitch, SpriteTab, Tab, Widget, WidgetState, cycle_frame, default_widget,
    filmstrip_frame, knob_rotation, rounded_rect, widget_for,
};
