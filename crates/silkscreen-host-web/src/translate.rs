//! DOM event fields → silkscreen [`RawKind`]s. Pure: the host reads the
//! fields off each event and hands them here, so the mapping is tested
//! without a browser.

use silkscreen::core::{Key, Modifiers, MouseButton, Point, RawKind, Size, Vec2};

// CSS pixels per wheel line, for `deltaMode` 0 (pixels): CSS pixels are
// already device-independent, so the device pixel ratio plays no part.
use silkscreen::core::input::PIXELS_PER_LINE;

/// `WheelEvent.deltaMode` values. Pixels (0) is what `wheel` assumes of
/// any mode it doesn't know.
#[cfg(test)]
pub(crate) const DOM_DELTA_PIXEL: u32 = 0;
pub(crate) const DOM_DELTA_LINE: u32 = 1;
pub(crate) const DOM_DELTA_PAGE: u32 = 2;

/// The modifier flags every DOM input event carries.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DomModifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
}

/// The fields of a `PointerEvent` or `MouseEvent` the editor needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DomPointer {
    pub client_x: f64,
    pub client_y: f64,
    /// `MouseEvent.button`: 0 left, 1 middle, 2 right, 3 back, 4 forward.
    pub button: i16,
    /// `MouseEvent.buttons`: the buttons held, as a bitmask; 0 when none.
    pub buttons: u16,
    pub mods: DomModifiers,
}

/// Where the canvas is on the page (`getBoundingClientRect`, CSS px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasBox {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Translates DOM events for one canvas.
///
/// It remembers the button pressed, so a press is always released, and
/// released with that button, however the browser ends it: `pointerup`
/// (whose `button` is the last released, after a chord), `pointercancel`
/// and `lostpointercapture` (whose `button` is -1), or a `pointermove`
/// with no buttons held after the release went missing.
#[derive(Clone, Debug)]
pub struct Translator {
    /// Logical size of the faceplate, which fills the canvas box.
    pub size: Size,
    /// Cmd is the primary modifier (macOS); Ctrl elsewhere.
    pub mac: bool,
    /// The button the editor was told is down, until it's told it's up.
    held: Option<MouseButton>,
}

impl Translator {
    pub fn new(size: Size, mac: bool) -> Self {
        Self {
            size,
            mac,
            held: None,
        }
    }

    /// Client coordinates → logical units. Follows the canvas box rather
    /// than assuming CSS px are logical units, so CSS scaling of the
    /// canvas is honoured.
    pub fn logical(&self, client_x: f64, client_y: f64, canvas: CanvasBox) -> Point {
        let sx = f64::from(self.size.w) / canvas.width.max(1.0);
        let sy = f64::from(self.size.h) / canvas.height.max(1.0);
        Point::new(
            ((client_x - canvas.left) * sx) as f32,
            ((client_y - canvas.top) * sy) as f32,
        )
    }

    pub fn mods(&self, m: DomModifiers) -> Modifiers {
        Modifiers::from_flags(m.shift, m.alt, m.ctrl, m.meta, self.mac)
    }

    /// A `pointerdown`. A press while one is still held means its release
    /// was lost: that release comes first.
    pub fn pointer_down(&mut self, p: DomPointer, canvas: CanvasBox) -> Vec<RawKind> {
        let mut out: Vec<RawKind> = self.pointer_up(p, canvas).into_iter().collect();
        let button = mouse_button(p.button);
        self.held = Some(button);
        out.push(RawKind::PointerDown {
            pos: self.logical(p.client_x, p.client_y, canvas),
            button,
            mods: self.mods(p.mods),
        });
        out
    }

    /// A `pointerup`, `pointercancel` or `lostpointercapture`: the release
    /// of the held button, or nothing if none is held (capture is lost
    /// after every `pointerup`).
    pub fn pointer_up(&mut self, p: DomPointer, canvas: CanvasBox) -> Option<RawKind> {
        let button = self.held.take()?;
        Some(RawKind::PointerUp {
            pos: self.logical(p.client_x, p.client_y, canvas),
            button,
            mods: self.mods(p.mods),
        })
    }

    /// A `pointermove`. A move with no buttons held while the editor
    /// thinks one is means the release went missing: it comes first.
    pub fn pointer_move(&mut self, p: DomPointer, canvas: CanvasBox) -> Vec<RawKind> {
        let mut out = Vec::with_capacity(2);
        if p.buttons == 0 {
            out.extend(self.pointer_up(p, canvas));
        }
        out.push(RawKind::PointerMove {
            pos: self.logical(p.client_x, p.client_y, canvas),
            mods: self.mods(p.mods),
        });
        out
    }

    /// A `wheel` event, normalized to lines. The DOM's positive deltas
    /// scroll down and right; silkscreen's positive `y` scrolls up (away
    /// from the user), as native hosts report, so both axes flip.
    pub fn wheel(
        &self,
        p: DomPointer,
        delta_x: f64,
        delta_y: f64,
        delta_mode: u32,
        canvas: CanvasBox,
    ) -> RawKind {
        let per_line = match delta_mode {
            DOM_DELTA_LINE => 1.0,
            // A page is the canvas's height, in lines.
            DOM_DELTA_PAGE => PIXELS_PER_LINE / canvas.height.max(PIXELS_PER_LINE),
            _ => PIXELS_PER_LINE,
        };
        RawKind::Wheel {
            pos: self.logical(p.client_x, p.client_y, canvas),
            delta: Vec2::new((-delta_x / per_line) as f32, (-delta_y / per_line) as f32),
            mods: self.mods(p.mods),
        }
    }

    /// A `keydown` (`pressed`) or `keyup`, from `KeyboardEvent.key`. Bare
    /// modifier presses are not keys to silkscreen.
    pub fn key(&self, key: &str, mods: DomModifiers, pressed: bool) -> Option<RawKind> {
        let key = Key::from_name(key)?;
        Some(RawKind::Key {
            key,
            mods: self.mods(mods),
            pressed,
        })
    }
}

/// Whether the browser's default action for `key` should be suppressed
/// while the canvas has focus: keys that would scroll the page, and Tab
/// while a panel (a text field, a menu) is open. Everything else, browser
/// shortcuts included, keeps its default.
pub fn suppress_key_default(key: &str, panel_open: bool) -> bool {
    match key {
        "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "PageUp" | "PageDown" | "Home"
        | "End" | " " | "Backspace" => true,
        "Tab" => panel_open,
        _ => false,
    }
}

pub fn mouse_button(button: i16) -> MouseButton {
    match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n.clamp(0, 255) as u8),
    }
}

/// Whether `platform` (`navigator.platform`) or `user_agent` names a Mac.
pub fn is_mac(platform: &str, user_agent: &str) -> bool {
    platform.starts_with("Mac") || (platform.is_empty() && user_agent.contains("Macintosh"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    const CANVAS: CanvasBox = CanvasBox {
        left: 100.0,
        top: 50.0,
        width: 300.0,
        height: 200.0,
    };

    fn t(mac: bool) -> Translator {
        Translator::new(Size::new(300.0, 200.0), mac)
    }

    fn at(x: f64, y: f64) -> DomPointer {
        DomPointer {
            client_x: x,
            client_y: y,
            ..DomPointer::default()
        }
    }

    #[test]
    fn positions_are_relative_to_the_canvas_box() {
        assert_eq!(
            t(true).pointer_move(at(160.0, 70.0), CANVAS),
            [RawKind::PointerMove {
                pos: Point::new(60.0, 20.0),
                mods: Modifiers::NONE
            }]
        );
    }

    #[test]
    fn a_css_scaled_canvas_still_maps_to_logical_units() {
        let doubled = CanvasBox {
            width: 600.0,
            height: 400.0,
            ..CANVAS
        };
        let [RawKind::PointerMove { pos, .. }] = t(true).pointer_move(at(160.0, 70.0), doubled)[..]
        else {
            panic!()
        };
        assert_eq!(pos, Point::new(30.0, 10.0));
    }

    #[test]
    fn positions_outside_the_canvas_are_kept_for_captured_drags() {
        let [RawKind::PointerMove { pos, .. }] = t(true).pointer_move(at(40.0, 300.0), CANVAS)[..]
        else {
            panic!()
        };
        assert_eq!(pos, Point::new(-60.0, 250.0));
    }

    #[test]
    fn buttons() {
        let mut p = at(100.0, 50.0);
        for (dom, ours) in [
            (0, MouseButton::Left),
            (1, MouseButton::Middle),
            (2, MouseButton::Right),
            (3, MouseButton::Other(3)),
            (4, MouseButton::Other(4)),
        ] {
            p.button = dom;
            let mut t = t(true);
            assert!(matches!(
                t.pointer_down(p, CANVAS)[..],
                [RawKind::PointerDown { button, .. }] if button == ours
            ));
            assert!(matches!(
                t.pointer_up(p, CANVAS),
                Some(RawKind::PointerUp { button, .. }) if button == ours
            ));
        }
    }

    fn up_button(kind: Option<&RawKind>) -> Option<MouseButton> {
        match kind {
            Some(RawKind::PointerUp { button, .. }) => Some(*button),
            _ => None,
        }
    }

    #[test]
    fn a_release_is_sent_once() {
        let mut t = t(true);
        t.pointer_down(at(110.0, 60.0), CANVAS);
        assert!(t.pointer_up(at(110.0, 60.0), CANVAS).is_some());
        // `lostpointercapture` follows every captured `pointerup`.
        assert!(t.pointer_up(at(110.0, 60.0), CANVAS).is_none());
    }

    #[test]
    fn a_chord_releases_the_button_first_pressed() {
        // Left down, right down, left up, right up: the browser's only
        // `pointerup` is the last, and it names the right button.
        let mut t = t(true);
        t.pointer_down(at(110.0, 60.0), CANVAS);
        let right = DomPointer {
            button: 2,
            ..at(120.0, 60.0)
        };
        assert_eq!(
            up_button(t.pointer_up(right, CANVAS).as_ref()),
            Some(MouseButton::Left)
        );
    }

    #[test]
    fn a_cancel_releases_the_held_button() {
        let mut t = t(true);
        t.pointer_down(at(110.0, 60.0), CANVAS);
        let cancel = DomPointer {
            button: -1,
            ..at(120.0, 60.0)
        };
        assert_eq!(
            up_button(t.pointer_up(cancel, CANVAS).as_ref()),
            Some(MouseButton::Left)
        );
    }

    #[test]
    fn a_move_with_no_buttons_releases_a_lost_press() {
        let mut t = t(true);
        t.pointer_down(at(110.0, 60.0), CANVAS);
        let dragging = DomPointer {
            buttons: 1,
            ..at(130.0, 60.0)
        };
        assert_eq!(t.pointer_move(dragging, CANVAS).len(), 1);
        let out = t.pointer_move(at(140.0, 60.0), CANVAS);
        assert_eq!(up_button(out.first()), Some(MouseButton::Left));
        assert!(matches!(out[1], RawKind::PointerMove { .. }));
        // Released once only.
        assert_eq!(t.pointer_move(at(150.0, 60.0), CANVAS).len(), 1);
    }

    #[test]
    fn a_press_while_held_releases_first() {
        let mut t = t(true);
        t.pointer_down(at(110.0, 60.0), CANVAS);
        let out = t.pointer_down(at(140.0, 60.0), CANVAS);
        assert_eq!(up_button(out.first()), Some(MouseButton::Left));
        assert!(matches!(out[1], RawKind::PointerDown { .. }));
    }

    #[test]
    fn cmd_is_meta_on_mac_and_ctrl_elsewhere() {
        let meta = DomModifiers {
            meta: true,
            ..DomModifiers::default()
        };
        let ctrl = DomModifiers {
            ctrl: true,
            ..DomModifiers::default()
        };
        assert!(t(true).mods(meta).cmd);
        assert!(!t(true).mods(ctrl).cmd);
        assert!(t(false).mods(ctrl).cmd);
        assert!(!t(false).mods(meta).cmd);
        assert!(t(true).mods(ctrl).ctrl);
    }

    #[test]
    fn wheel_normalizes_to_lines_with_up_positive() {
        let wheel = |dx, dy, mode| match t(true).wheel(at(100.0, 50.0), dx, dy, mode, CANVAS) {
            RawKind::Wheel { delta, .. } => delta,
            _ => panic!(),
        };
        // Scrolling down 32 px is two lines down.
        assert_eq!(wheel(0.0, 32.0, DOM_DELTA_PIXEL), Vec2::new(0.0, -2.0));
        assert_eq!(wheel(0.0, -3.0, DOM_DELTA_LINE), Vec2::new(0.0, 3.0));
        assert_eq!(wheel(16.0, 0.0, DOM_DELTA_PIXEL), Vec2::new(-1.0, 0.0));
        // A page is the canvas height: 200 px = 12.5 lines.
        assert_eq!(wheel(0.0, 1.0, DOM_DELTA_PAGE), Vec2::new(0.0, -12.5));
    }

    #[test]
    fn keys() {
        let none = DomModifiers::default();
        assert_eq!(
            t(true).key("z", none, true),
            Some(RawKind::Key {
                key: Key::Char('z'),
                mods: Modifiers::NONE,
                pressed: true
            })
        );
        assert!(matches!(
            t(true).key("ArrowUp", none, false),
            Some(RawKind::Key {
                key: Key::ArrowUp,
                pressed: false,
                ..
            })
        ));
        assert!(matches!(
            t(true).key("é", none, true),
            Some(RawKind::Key {
                key: Key::Char('é'),
                ..
            })
        ));
        assert!(matches!(
            t(true).key("F5", none, true),
            Some(RawKind::Key {
                key: Key::Other,
                ..
            })
        ));
        assert_eq!(t(true).key("Shift", none, true), None);
        assert_eq!(t(true).key("Meta", none, true), None);
    }

    #[test]
    fn only_scrolling_keys_and_tab_in_a_panel_lose_their_default() {
        assert!(suppress_key_default("ArrowDown", false));
        assert!(suppress_key_default(" ", false));
        assert!(!suppress_key_default("Tab", false));
        assert!(suppress_key_default("Tab", true));
        assert!(!suppress_key_default("r", false));
        assert!(!suppress_key_default("z", true));
    }

    #[test]
    fn mac_detection() {
        assert!(is_mac("MacIntel", ""));
        assert!(!is_mac("Win32", "Mozilla/5.0 (Windows NT 10.0)"));
        assert!(is_mac(
            "",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"
        ));
    }
}
