//! The bridge between a silkscreen editor and an audio engine ([design/engine-bridge.md]).
//!
//! The protocol is fixed: [`EngineEvent`], which always encodes to a
//! 16-byte record ([`codec`]). Transports are swappable: native rings and
//! atomics, or a `SharedArrayBuffer` on the web. Engine code depends only on
//! this crate.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

#![cfg_attr(not(feature = "std"), no_std)]

pub mod codec;
mod engine;
mod event;
pub mod fp;
mod link;
#[cfg(feature = "std")]
mod native;
mod note;
#[cfg(feature = "std")]
pub mod sab;
#[cfg(feature = "std")]
mod side;
#[cfg(feature = "std")]
pub mod telemetry;
#[cfg(feature = "testing")]
pub mod testing;
mod transport;
#[cfg(feature = "std")]
pub mod web_engine;
#[cfg(feature = "std")]
mod words;
#[cfg(feature = "std")]
pub use words::ReaderWords;

/// UI → engine events a bridge's ring holds, for hosts with no reason
/// to choose: far more than a drag sends in an audio thread's slowest
/// block.
pub const RING_CAPACITY: usize = 1024;

pub use engine::Engine;
pub use event::{EngineEvent, ParamIndex};
pub use fp::DenormalGuard;
#[cfg(feature = "std")]
pub use link::RecordingLink;
pub use link::{EngineLink, NullLink};
#[cfg(feature = "std")]
pub use native::{EngineEnd, UiLink, bridge, bridge_with_frame};
pub use note::NoteEvent;
#[cfg(feature = "std")]
pub use side::{EngineSide, patch_loaded};
#[cfg(feature = "std")]
pub use telemetry::{Peaks, TelemetryReader, TelemetryTap, telemetry, telemetry_with_frame};
pub use transport::Transport;
