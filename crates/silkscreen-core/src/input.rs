//! Host-neutral raw input ([design/input.md]).
//!
//! Hosts (CLAP window, browser canvas, test harness) translate their native
//! events into [`RawEvent`]s. Positions are in **logical** units: the host
//! divides physical coordinates by the effective scale. Timestamps are
//! milliseconds on any monotonic clock the host chooses; only differences
//! matter.
//!
//! [design/input.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/input.md

use std::path::PathBuf;

use crate::geom::{Point, Vec2};

/// Device-independent pixels per wheel line, for hosts that report
/// pixel-precise (trackpad) scrolling and must convert it into lines.
pub const PIXELS_PER_LINE: f64 = 16.0;

/// Keyboard modifier state, sampled per event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    /// The platform's primary command modifier: Cmd on macOS, Ctrl
    /// elsewhere. Hosts set this (on Windows/Linux it is set together with
    /// `ctrl`); bindings test `cmd` for shortcuts like reset-to-default.
    pub cmd: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        alt: false,
        ctrl: false,
        cmd: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    pub const CMD: Modifiers = Modifiers {
        cmd: true,
        ..Modifiers::NONE
    };

    /// From the four flags hosts report: `cmd` is Meta on macOS (`mac`),
    /// Ctrl elsewhere.
    #[allow(clippy::fn_params_excessive_bools)]
    pub const fn from_flags(shift: bool, alt: bool, ctrl: bool, meta: bool, mac: bool) -> Self {
        Modifiers {
            shift,
            alt,
            ctrl,
            cmd: if mac { meta } else { ctrl },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// Keys silkscreen cares about. Keyboard delivery inside plugin windows is
/// best-effort (design/input.md), so nothing essential depends on this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Other,
}

impl Key {
    /// A key from its name as the DOM's `KeyboardEvent.key` spells it,
    /// as `keyboard-types` does too: `Enter`, `ArrowUp`, a single
    /// character. `None` for a bare modifier press, which isn't a key to
    /// silkscreen.
    pub fn from_name(name: &str) -> Option<Key> {
        Some(match name {
            "Enter" => Key::Enter,
            "Escape" => Key::Escape,
            "Backspace" => Key::Backspace,
            "Delete" => Key::Delete,
            "Tab" => Key::Tab,
            "ArrowUp" => Key::ArrowUp,
            "ArrowDown" => Key::ArrowDown,
            "ArrowLeft" => Key::ArrowLeft,
            "ArrowRight" => Key::ArrowRight,
            "Home" => Key::Home,
            "End" => Key::End,
            "PageUp" => Key::PageUp,
            "PageDown" => Key::PageDown,
            "Shift" | "Control" | "Alt" | "Meta" | "AltGraph" | "CapsLock" | "Fn" => return None,
            s => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Char(c),
                    _ => Key::Other,
                }
            }
        })
    }
}

/// What was dropped onto the editor from outside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropPayload {
    /// Native hosts: filesystem paths.
    Paths(Vec<PathBuf>),
    /// Web host: handles to the browser `File`s of one drop, resolved
    /// with `silkscreen_host_web::take_dropped_file` while the drop is
    /// handled. The next drop releases any not taken.
    Files(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawEvent {
    pub time_ms: f64,
    pub kind: RawKind,
}

impl RawEvent {
    pub fn new(time_ms: f64, kind: RawKind) -> Self {
        Self { time_ms, kind }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RawKind {
    PointerDown {
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
    },
    PointerUp {
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
    },
    PointerMove {
        pos: Point,
        mods: Modifiers,
    },
    /// The pointer left the window (not delivered while captured mid-drag
    /// on hosts that capture; harmless if it is).
    PointerLeave,
    /// Scroll, normalized to lines; positive `y` is away from the user
    /// (scroll up).
    Wheel {
        pos: Point,
        delta: Vec2,
        mods: Modifiers,
    },
    Key {
        key: Key,
        mods: Modifiers,
        pressed: bool,
    },
    /// The editor window lost keyboard focus.
    FocusLost,
    Drop {
        pos: Point,
        payload: DropPayload,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_by_their_dom_names() {
        assert_eq!(Key::from_name("ArrowUp"), Some(Key::ArrowUp));
        assert_eq!(Key::from_name("PageDown"), Some(Key::PageDown));
        assert_eq!(Key::from_name("é"), Some(Key::Char('é')));
        assert_eq!(Key::from_name("F5"), Some(Key::Other));
        assert_eq!(Key::from_name("Shift"), None, "a bare modifier");
    }

    #[test]
    fn cmd_is_meta_on_a_mac_and_ctrl_elsewhere() {
        let ctrl = Modifiers::from_flags(false, false, true, false, true);
        assert!(ctrl.ctrl && !ctrl.cmd);
        assert!(Modifiers::from_flags(false, false, false, true, true).cmd);
        assert!(Modifiers::from_flags(false, false, true, false, false).cmd);
        assert!(!Modifiers::from_flags(true, true, false, true, false).cmd);
    }
}
