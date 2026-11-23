//! baseview events → silkscreen [`RawKind`]s. Pure, so it is unit-tested
//! without a window.

use std::path::PathBuf;

use baseview::{DropData, Event, MouseButton as BvButton, MouseEvent, ScrollDelta, WindowEvent};
use keyboard_types::{Key as KtKey, KeyState, KeyboardEvent, Modifiers as KtModifiers};
use silkscreen::core::{DropPayload, Key, Modifiers, MouseButton, Point, RawKind, Vec2};

use silkscreen::core::input::PIXELS_PER_LINE;

/// Translates baseview events, remembering the last pointer position
/// (baseview's button events carry none).
#[derive(Debug, Clone, Copy)]
pub struct Translator {
    /// Physical pixels per silkscreen logical unit: display scale × zoom.
    pub effective_scale: f64,
    /// Physical pixels per OS logical point (the display scale alone).
    pub display_scale: f64,
    last_pos: Point,
}

impl Translator {
    pub fn new(display_scale: f64, effective_scale: f64) -> Self {
        Self {
            effective_scale,
            display_scale,
            last_pos: Point::new(-1.0, -1.0),
        }
    }

    fn logical(&self, x: f64, y: f64) -> Point {
        Point::new(
            (x / self.effective_scale) as f32,
            (y / self.effective_scale) as f32,
        )
    }

    /// The silkscreen event for `event`, if it has one.
    pub fn translate(&mut self, event: &Event) -> Option<RawKind> {
        match event {
            Event::Mouse(m) => self.mouse(m),
            Event::Keyboard(k) => keyboard(k),
            Event::Window(WindowEvent::Unfocused) => Some(RawKind::FocusLost),
            _ => None,
        }
    }

    fn mouse(&mut self, event: &MouseEvent) -> Option<RawKind> {
        match event {
            MouseEvent::CursorMoved {
                position,
                modifiers,
            } => {
                self.last_pos = self.logical(position.x, position.y);
                Some(RawKind::PointerMove {
                    pos: self.last_pos,
                    mods: mods(*modifiers),
                })
            }
            MouseEvent::ButtonPressed { button, modifiers } => Some(RawKind::PointerDown {
                pos: self.last_pos,
                button: mouse_button(*button),
                mods: mods(*modifiers),
            }),
            MouseEvent::ButtonReleased { button, modifiers } => Some(RawKind::PointerUp {
                pos: self.last_pos,
                button: mouse_button(*button),
                mods: mods(*modifiers),
            }),
            MouseEvent::WheelScrolled { delta, modifiers } => {
                let delta = match *delta {
                    ScrollDelta::Lines { x, y } => Vec2::new(x, y),
                    ScrollDelta::Pixels { x, y } => {
                        let per_line = PIXELS_PER_LINE * self.display_scale;
                        Vec2::new(
                            (f64::from(x) / per_line) as f32,
                            (f64::from(y) / per_line) as f32,
                        )
                    }
                };
                Some(RawKind::Wheel {
                    pos: self.last_pos,
                    delta,
                    mods: mods(*modifiers),
                })
            }
            MouseEvent::CursorLeft => Some(RawKind::PointerLeave),
            MouseEvent::DragDropped { position, data, .. } => {
                let pos = self.logical(position.x, position.y);
                match data {
                    DropData::Files(paths) => Some(RawKind::Drop {
                        pos,
                        payload: DropPayload::Paths(paths.iter().map(PathBuf::clone).collect()),
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

pub fn mods(m: KtModifiers) -> Modifiers {
    Modifiers::from_flags(
        m.contains(KtModifiers::SHIFT),
        m.contains(KtModifiers::ALT),
        m.contains(KtModifiers::CONTROL),
        m.contains(KtModifiers::META),
        cfg!(target_os = "macos"),
    )
}

fn mouse_button(b: BvButton) -> MouseButton {
    match b {
        BvButton::Left => MouseButton::Left,
        BvButton::Right => MouseButton::Right,
        BvButton::Middle => MouseButton::Middle,
        BvButton::Back => MouseButton::Other(3),
        BvButton::Forward => MouseButton::Other(4),
        BvButton::Other(n) => MouseButton::Other(n),
        _ => MouseButton::Other(255),
    }
}

fn keyboard(event: &KeyboardEvent) -> Option<RawKind> {
    // keyboard-types names keys as the DOM does.
    let key = match &event.key {
        KtKey::Character(s) => Key::from_name(s)?,
        KtKey::Named(named) => Key::from_name(&named.to_string())?,
    };
    Some(RawKind::Key {
        key,
        mods: mods(event.modifiers),
        pressed: event.state == KeyState::Down,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use baseview::dpi::PhysicalPosition;
    use keyboard_types::Code;
    use keyboard_types::NamedKey;

    fn moved(x: f64, y: f64, modifiers: KtModifiers) -> Event {
        Event::Mouse(MouseEvent::CursorMoved {
            position: PhysicalPosition::new(x, y),
            modifiers,
        })
    }

    #[test]
    fn positions_divide_by_effective_scale() {
        // 2x display, 150% zoom: 3 physical px per logical unit.
        let mut t = Translator::new(2.0, 3.0);
        let ev = t.translate(&moved(300.0, 150.0, KtModifiers::empty()));
        assert_eq!(
            ev,
            Some(RawKind::PointerMove {
                pos: Point::new(100.0, 50.0),
                mods: Modifiers::NONE
            })
        );
    }

    #[test]
    fn buttons_use_the_last_pointer_position() {
        let mut t = Translator::new(1.0, 1.0);
        t.translate(&moved(40.0, 60.0, KtModifiers::empty()));
        let down = t.translate(&Event::Mouse(MouseEvent::ButtonPressed {
            button: BvButton::Left,
            modifiers: KtModifiers::SHIFT,
        }));
        assert_eq!(
            down,
            Some(RawKind::PointerDown {
                pos: Point::new(40.0, 60.0),
                button: MouseButton::Left,
                mods: Modifiers::SHIFT
            })
        );
        let up = t.translate(&Event::Mouse(MouseEvent::ButtonReleased {
            button: BvButton::Right,
            modifiers: KtModifiers::empty(),
        }));
        assert!(matches!(
            up,
            Some(RawKind::PointerUp {
                button: MouseButton::Right,
                ..
            })
        ));
    }

    #[test]
    fn cmd_is_the_platform_primary_modifier() {
        let meta = mods(KtModifiers::META);
        let ctrl = mods(KtModifiers::CONTROL);
        if cfg!(target_os = "macos") {
            assert!(meta.cmd && !ctrl.cmd);
        } else {
            assert!(!meta.cmd && ctrl.cmd);
        }
        assert!(ctrl.ctrl);
    }

    #[test]
    fn wheel_lines_pass_through_and_pixels_convert() {
        let mut t = Translator::new(2.0, 2.0);
        let wheel = |delta| {
            Event::Mouse(MouseEvent::WheelScrolled {
                delta,
                modifiers: KtModifiers::empty(),
            })
        };
        let lines = t.translate(&wheel(ScrollDelta::Lines { x: 0.0, y: 2.0 }));
        assert!(
            matches!(lines, Some(RawKind::Wheel { delta, .. }) if delta == Vec2::new(0.0, 2.0))
        );
        // 64 physical px at 2x display = 32 points = 2 lines.
        let px = t.translate(&wheel(ScrollDelta::Pixels { x: 0.0, y: 64.0 }));
        assert!(matches!(px, Some(RawKind::Wheel { delta, .. }) if delta == Vec2::new(0.0, 2.0)));
    }

    #[test]
    fn leave_focus_and_drop() {
        let mut t = Translator::new(1.0, 2.0);
        assert_eq!(
            t.translate(&Event::Mouse(MouseEvent::CursorLeft)),
            Some(RawKind::PointerLeave)
        );
        assert_eq!(
            t.translate(&Event::Window(WindowEvent::Unfocused)),
            Some(RawKind::FocusLost)
        );
        assert_eq!(t.translate(&Event::Window(WindowEvent::Focused)), None);
        let drop = t.translate(&Event::Mouse(MouseEvent::DragDropped {
            position: PhysicalPosition::new(20.0, 10.0),
            modifiers: KtModifiers::empty(),
            data: DropData::Files(vec!["/a.wav".into()]),
        }));
        assert_eq!(
            drop,
            Some(RawKind::Drop {
                pos: Point::new(10.0, 5.0),
                payload: DropPayload::Paths(vec!["/a.wav".into()])
            })
        );
    }

    #[test]
    fn keys() {
        let mut t = Translator::new(1.0, 1.0);
        let mut ev = KeyboardEvent::key_down(KtKey::Character("z".into()), Code::KeyZ);
        ev.modifiers = KtModifiers::META;
        assert!(matches!(
            t.translate(&Event::Keyboard(ev)),
            Some(RawKind::Key {
                key: Key::Char('z'),
                pressed: true,
                ..
            })
        ));
        let ev = KeyboardEvent::key_down(KtKey::Named(NamedKey::ArrowUp), Code::ArrowUp);
        assert!(matches!(
            t.translate(&Event::Keyboard(ev)),
            Some(RawKind::Key {
                key: Key::ArrowUp,
                ..
            })
        ));
        let ev = KeyboardEvent::key_down(KtKey::Named(NamedKey::Shift), Code::ShiftLeft);
        assert_eq!(t.translate(&Event::Keyboard(ev)), None);
    }
}
