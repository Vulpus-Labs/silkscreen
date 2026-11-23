//! Gesture recognition: [`RawEvent`]s in, per-control [`ControlEvent`]s out
//! ([design/input.md]).
//!
//! One recognizer serves every host. It is a pure state machine: time comes
//! from event timestamps and [`GestureRecognizer::tick`], never a clock.
//!
//! Only the left button clicks and drags. Other buttons produce
//! `Press`/`Release` only, so bindings can implement context actions.
//!
//! [design/input.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/input.md

use crate::geom::{Point, Vec2};
use crate::input::{DropPayload, Key, Modifiers, MouseButton, RawEvent, RawKind};
use crate::layout::{ControlId, Layout};

/// Thresholds, in logical units and milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureConfig {
    /// Movement from the press point beyond which a press becomes a drag.
    pub drag_threshold: f32,
    /// Maximum time from one click to the next press for them to count as a
    /// multi-click on the same control.
    pub double_click_ms: f64,
    /// Stillness over a control before [`ControlEvent::HoverDwell`].
    pub dwell_ms: f64,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            drag_threshold: 3.0,
            double_click_ms: 400.0,
            dwell_ms: 500.0,
        }
    }
}

/// An input event resolved to a control.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlEvent {
    Press {
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
    },
    Release {
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
    },
    /// A left press and release without crossing the drag threshold.
    /// `count` is 2 for a double-click, 3 for a triple, and so on.
    Click {
        count: u8,
        mods: Modifiers,
    },
    /// The drag threshold was crossed; `pos` is where the press started.
    DragStart {
        pos: Point,
        mods: Modifiers,
    },
    /// `delta` is the movement since the previous `Drag` (or since the press
    /// point, for the first). Modifiers are sampled per event.
    Drag {
        delta: Vec2,
        pos: Point,
        mods: Modifiers,
    },
    /// `over` is the control under the pointer at release, for
    /// control-to-control drag-and-drop. `None` if the drag was cancelled.
    DragEnd {
        over: Option<ControlId>,
        mods: Modifiers,
    },
    /// Scroll in lines; positive `y` is scroll up.
    Wheel {
        delta: Vec2,
        mods: Modifiers,
    },
    HoverEnter,
    /// The pointer moved over the control with no button held, after
    /// its `HoverEnter`: for controls with handles of their own, such
    /// as a canvas ([design/faceplates.md]).
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    Hover {
        pos: Point,
    },
    HoverLeave,
    /// The pointer has rested on the control for [`GestureConfig::dwell_ms`].
    /// Fires once per rest.
    HoverDwell,
    Key {
        key: Key,
        mods: Modifiers,
        pressed: bool,
    },
    Drop {
        payload: DropPayload,
    },
}

#[derive(Clone, Copy, Debug)]
enum Pointer {
    Idle,
    /// A non-left button is down, or a left press missed every control.
    /// Waits for the release, tracking nothing.
    Held {
        target: Option<ControlId>,
        button: MouseButton,
    },
    Pressed {
        target: ControlId,
        origin: Point,
        time_ms: f64,
    },
    Dragging {
        target: ControlId,
        last: Point,
    },
}

#[derive(Clone, Copy, Debug)]
struct LastClick {
    target: ControlId,
    time_ms: f64,
    count: u8,
}

/// Tracks pointer, hover, focus and multi-click state for one editor view.
#[derive(Debug)]
pub struct GestureRecognizer {
    config: GestureConfig,
    pointer: Pointer,
    hover: Option<ControlId>,
    /// When the pointer last moved, if a dwell is pending.
    dwell_since: Option<f64>,
    focus: Option<ControlId>,
    last_click: Option<LastClick>,
    /// Each page group's shown page: controls on other pages aren't hit.
    shown: Vec<u16>,
}

/// Output buffer: events paired with the control they're addressed to.
pub type GestureOutput = Vec<(ControlId, ControlEvent)>;

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new(GestureConfig::default())
    }
}

impl GestureRecognizer {
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            pointer: Pointer::Idle,
            hover: None,
            dwell_since: None,
            focus: None,
            last_click: None,
            shown: Vec::new(),
        }
    }

    /// Which page each page group shows ([design/faceplates.md]): only controls off
    /// pages or on shown pages are hit. A hover or focus on a control
    /// that has gone is dropped.
    ///
    /// [design/faceplates.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/faceplates.md
    pub fn set_shown_pages(&mut self, layout: &Layout, shown: &[u16]) {
        self.shown.clear();
        self.shown.extend_from_slice(shown);
        let gone = |id: Option<ControlId>| id.is_some_and(|i| !layout.control(i).is_shown(shown));
        if gone(self.hover) {
            self.hover = None;
            self.dwell_since = None;
        }
        if gone(self.focus) {
            self.focus = None;
        }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.config
    }

    /// The control with keyboard focus (the last one pressed).
    pub fn focus(&self) -> Option<ControlId> {
        self.focus
    }

    /// The control under the pointer, if no button is held.
    pub fn hover(&self) -> Option<ControlId> {
        self.hover
    }

    /// The control currently being dragged or pressed.
    pub fn active(&self) -> Option<ControlId> {
        match self.pointer {
            Pointer::Pressed { target, .. } | Pointer::Dragging { target, .. } => Some(target),
            Pointer::Held { target, .. } => target,
            Pointer::Idle => None,
        }
    }

    /// Feed one raw event; resolved events are appended to `out`.
    pub fn handle(&mut self, layout: &Layout, ev: &RawEvent, out: &mut GestureOutput) {
        let now = ev.time_ms;
        match &ev.kind {
            RawKind::PointerDown { pos, button, mods } => {
                self.down(layout, now, *pos, *button, *mods, out);
            }
            RawKind::PointerUp { pos, button, mods } => {
                self.up(layout, now, *pos, *button, *mods, out);
            }
            RawKind::PointerMove { pos, mods } => self.moved(layout, now, *pos, *mods, out),
            RawKind::PointerLeave => {
                if matches!(self.pointer, Pointer::Idle) {
                    self.set_hover(None, now, out);
                }
            }
            RawKind::Wheel { pos, delta, mods } => {
                if let Some(id) = layout.hit_test_shown(*pos, &self.shown) {
                    out.push((
                        id,
                        ControlEvent::Wheel {
                            delta: *delta,
                            mods: *mods,
                        },
                    ));
                }
            }
            RawKind::Key { key, mods, pressed } => {
                if let Some(id) = self.focus {
                    out.push((
                        id,
                        ControlEvent::Key {
                            key: *key,
                            mods: *mods,
                            pressed: *pressed,
                        },
                    ));
                }
            }
            RawKind::FocusLost => {
                self.focus = None;
                // A lost focus can swallow the release; never leave a drag
                // (and so a host gesture) open.
                self.cancel(out);
            }
            RawKind::Drop { pos, payload } => {
                if let Some(id) = layout.hit_test_shown(*pos, &self.shown) {
                    out.push((
                        id,
                        ControlEvent::Drop {
                            payload: payload.clone(),
                        },
                    ));
                }
            }
        }
    }

    /// Advance time with no input; fires pending dwells.
    pub fn tick(&mut self, now_ms: f64, out: &mut GestureOutput) {
        if let (Some(since), Some(id), Pointer::Idle) = (self.dwell_since, self.hover, self.pointer)
            && now_ms - since >= self.config.dwell_ms
        {
            self.dwell_since = None;
            out.push((id, ControlEvent::HoverDwell));
        }
    }

    /// Abandon any press or drag in progress, ending an open drag with
    /// `DragEnd { over: None }`. Hosts call this when the view is torn down.
    pub fn cancel(&mut self, out: &mut GestureOutput) {
        if let Pointer::Dragging { target, .. } = self.pointer {
            out.push((
                target,
                ControlEvent::DragEnd {
                    over: None,
                    mods: Modifiers::NONE,
                },
            ));
        }
        self.pointer = Pointer::Idle;
    }

    fn down(
        &mut self,
        layout: &Layout,
        now: f64,
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
        out: &mut GestureOutput,
    ) {
        if !matches!(self.pointer, Pointer::Idle) {
            // Second button while one is held: ignore until all released.
            return;
        }
        self.dwell_since = None;
        let target = layout.hit_test_shown(pos, &self.shown);
        self.focus = target;
        let Some(id) = target else {
            self.pointer = Pointer::Held {
                target: None,
                button,
            };
            return;
        };
        out.push((id, ControlEvent::Press { pos, button, mods }));
        self.pointer = if button == MouseButton::Left {
            Pointer::Pressed {
                target: id,
                origin: pos,
                time_ms: now,
            }
        } else {
            Pointer::Held {
                target: Some(id),
                button,
            }
        };
    }

    fn up(
        &mut self,
        layout: &Layout,
        now: f64,
        pos: Point,
        button: MouseButton,
        mods: Modifiers,
        out: &mut GestureOutput,
    ) {
        match self.pointer {
            Pointer::Idle => return,
            Pointer::Held {
                target,
                button: held,
            } => {
                if held != button {
                    return;
                }
                if let Some(id) = target {
                    out.push((id, ControlEvent::Release { pos, button, mods }));
                }
            }
            Pointer::Pressed {
                target, time_ms, ..
            } => {
                if button != MouseButton::Left {
                    return;
                }
                out.push((target, ControlEvent::Release { pos, button, mods }));
                let count = match self.last_click {
                    Some(last)
                        if last.target == target
                            && time_ms - last.time_ms <= self.config.double_click_ms =>
                    {
                        last.count.saturating_add(1)
                    }
                    _ => 1,
                };
                self.last_click = Some(LastClick {
                    target,
                    time_ms: now,
                    count,
                });
                out.push((target, ControlEvent::Click { count, mods }));
            }
            Pointer::Dragging { target, .. } => {
                if button != MouseButton::Left {
                    return;
                }
                out.push((target, ControlEvent::Release { pos, button, mods }));
                out.push((
                    target,
                    ControlEvent::DragEnd {
                        over: layout.hit_test_shown(pos, &self.shown),
                        mods,
                    },
                ));
                self.last_click = None;
            }
        }
        self.pointer = Pointer::Idle;
        // Hover was frozen while the button was held; catch up.
        self.set_hover(layout.hit_test_shown(pos, &self.shown), now, out);
    }

    fn moved(
        &mut self,
        layout: &Layout,
        now: f64,
        pos: Point,
        mods: Modifiers,
        out: &mut GestureOutput,
    ) {
        match self.pointer {
            Pointer::Idle => {
                self.set_hover(layout.hit_test_shown(pos, &self.shown), now, out);
                if let Some(id) = self.hover {
                    out.push((id, ControlEvent::Hover { pos }));
                    self.dwell_since = Some(now);
                }
            }
            Pointer::Held { .. } => {}
            Pointer::Pressed { target, origin, .. } => {
                if pos.distance(origin) > self.config.drag_threshold {
                    out.push((target, ControlEvent::DragStart { pos: origin, mods }));
                    out.push((
                        target,
                        ControlEvent::Drag {
                            delta: pos - origin,
                            pos,
                            mods,
                        },
                    ));
                    self.pointer = Pointer::Dragging { target, last: pos };
                }
            }
            Pointer::Dragging { target, last } => {
                out.push((
                    target,
                    ControlEvent::Drag {
                        delta: pos - last,
                        pos,
                        mods,
                    },
                ));
                self.pointer = Pointer::Dragging { target, last: pos };
            }
        }
    }

    fn set_hover(&mut self, next: Option<ControlId>, now: f64, out: &mut GestureOutput) {
        if next == self.hover {
            return;
        }
        if let Some(prev) = self.hover {
            out.push((prev, ControlEvent::HoverLeave));
        }
        if let Some(id) = next {
            out.push((id, ControlEvent::HoverEnter));
        }
        self.hover = next;
        self.dwell_since = next.map(|_| now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Rect, Size};
    use crate::layout::{ControlKind, ControlSpec};

    const A: ControlId = ControlId(0);
    const B: ControlId = ControlId(1);

    static CONTROLS: [ControlSpec; 2] = [
        ControlSpec::new(A, "a", ControlKind::Knob, Rect::new(0.0, 0.0, 50.0, 50.0)),
        ControlSpec::new(B, "b", ControlKind::Knob, Rect::new(100.0, 0.0, 50.0, 50.0)),
    ];
    static LAYOUT: Layout = Layout::new(Size::new(200.0, 100.0), &CONTROLS);

    const IN_A: Point = Point::new(25.0, 25.0);
    const IN_B: Point = Point::new(125.0, 25.0);
    const BARE: Point = Point::new(75.0, 75.0);

    /// Scripted driver: each call feeds one event and returns what it emitted.
    struct Script {
        g: GestureRecognizer,
        t: f64,
    }

    impl Script {
        fn new() -> Self {
            Self {
                g: GestureRecognizer::default(),
                t: 0.0,
            }
        }
        fn at(&mut self, t: f64) -> &mut Self {
            self.t = t;
            self
        }
        fn feed(&mut self, kind: RawKind) -> GestureOutput {
            let mut out = Vec::new();
            self.g
                .handle(&LAYOUT, &RawEvent::new(self.t, kind), &mut out);
            out
        }
        fn down(&mut self, pos: Point) -> GestureOutput {
            self.down_with(pos, Modifiers::NONE)
        }
        fn down_with(&mut self, pos: Point, mods: Modifiers) -> GestureOutput {
            self.feed(RawKind::PointerDown {
                pos,
                button: MouseButton::Left,
                mods,
            })
        }
        fn up(&mut self, pos: Point) -> GestureOutput {
            self.feed(RawKind::PointerUp {
                pos,
                button: MouseButton::Left,
                mods: Modifiers::NONE,
            })
        }
        fn mv(&mut self, pos: Point) -> GestureOutput {
            self.mv_with(pos, Modifiers::NONE)
        }
        fn mv_with(&mut self, pos: Point, mods: Modifiers) -> GestureOutput {
            self.feed(RawKind::PointerMove { pos, mods })
        }
        fn tick(&mut self, t: f64) -> GestureOutput {
            self.t = t;
            let mut out = Vec::new();
            self.g.tick(t, &mut out);
            out
        }
        /// Press and release at `pos`, returning only the `Click` event.
        fn click(&mut self, pos: Point) -> Option<(ControlId, ControlEvent)> {
            self.down(pos);
            self.up(pos)
                .into_iter()
                .find(|(_, e)| matches!(e, ControlEvent::Click { .. }))
        }
    }

    fn kinds(out: &GestureOutput) -> Vec<&'static str> {
        out.iter()
            .map(|(_, e)| match e {
                ControlEvent::Press { .. } => "Press",
                ControlEvent::Release { .. } => "Release",
                ControlEvent::Click { .. } => "Click",
                ControlEvent::DragStart { .. } => "DragStart",
                ControlEvent::Drag { .. } => "Drag",
                ControlEvent::DragEnd { .. } => "DragEnd",
                ControlEvent::Wheel { .. } => "Wheel",
                ControlEvent::HoverEnter => "HoverEnter",
                ControlEvent::Hover { .. } => "Hover",
                ControlEvent::HoverLeave => "HoverLeave",
                ControlEvent::HoverDwell => "HoverDwell",
                ControlEvent::Key { .. } => "Key",
                ControlEvent::Drop { .. } => "Drop",
            })
            .collect()
    }

    const fn click(count: u8) -> ControlEvent {
        ControlEvent::Click {
            count,
            mods: Modifiers::NONE,
        }
    }

    #[test]
    fn press_release_is_click() {
        let mut s = Script::new();
        s.mv(IN_A);
        let mut out = s.down(IN_A);
        out.extend(s.at(50.0).up(IN_A));
        assert_eq!(kinds(&out), ["Press", "Release", "Click"]);
        assert!(out.iter().all(|(id, _)| *id == A));
        assert_eq!(out[2].1, click(1));
    }

    #[test]
    fn double_click_on_same_control_within_window() {
        let mut s = Script::new();
        assert_eq!(s.click(IN_A), Some((A, click(1))));
        s.at(300.0);
        assert_eq!(s.click(IN_A), Some((A, click(2))));
    }

    #[test]
    fn slow_second_click_is_single() {
        let mut s = Script::new();
        s.click(IN_A);
        s.at(401.0);
        assert_eq!(s.click(IN_A), Some((A, click(1))));
    }

    #[test]
    fn second_click_on_other_control_is_single() {
        let mut s = Script::new();
        s.click(IN_A);
        s.at(100.0);
        assert_eq!(s.click(IN_B), Some((B, click(1))));
        // ...and doesn't count towards a double on A either.
        s.at(200.0);
        assert_eq!(s.click(IN_A), Some((A, click(1))));
    }

    #[test]
    fn jitter_under_threshold_is_still_click() {
        let mut s = Script::new();
        s.mv(IN_A);
        let mut out = s.down(IN_A);
        out.extend(s.mv(Point::new(27.0, 27.0))); // ~2.83 px
        out.extend(s.up(Point::new(27.0, 27.0)));
        assert_eq!(kinds(&out), ["Press", "Release", "Click"]);
    }

    #[test]
    fn crossing_threshold_starts_drag_from_press_point() {
        let mut s = Script::new();
        s.mv(IN_A);
        s.down(IN_A);
        let out = s.mv(Point::new(25.0, 20.0));
        assert_eq!(kinds(&out), ["DragStart", "Drag"]);
        assert_eq!(
            out[0].1,
            ControlEvent::DragStart {
                pos: IN_A,
                mods: Modifiers::NONE
            }
        );
        assert!(
            matches!(out[1].1, ControlEvent::Drag { delta, .. } if delta == Vec2::new(0.0, -5.0))
        );

        let out = s.mv(Point::new(25.0, 12.0));
        assert!(
            matches!(out[0].1, ControlEvent::Drag { delta, .. } if delta == Vec2::new(0.0, -8.0))
        );

        let out = s.up(Point::new(25.0, 12.0));
        assert_eq!(kinds(&out), ["Release", "DragEnd"]);
    }

    #[test]
    fn drag_is_captured_outside_controls_and_window() {
        let mut s = Script::new();
        s.down(IN_A);
        s.mv(Point::new(25.0, 10.0));
        // Over bare faceplate, then outside the window entirely.
        let out = s.mv(BARE);
        assert_eq!(out[0].0, A);
        assert!(s.feed(RawKind::PointerLeave).is_empty());
        let out = s.mv(Point::new(-500.0, 900.0));
        assert_eq!(kinds(&out), ["Drag"]);
        assert_eq!(out[0].0, A);
        // No hover events while dragging, even over B.
        assert_eq!(kinds(&s.mv(IN_B)), ["Drag"]);
    }

    #[test]
    fn drag_end_reports_drop_target() {
        let mut s = Script::new();
        s.down(IN_A);
        s.mv(IN_B);
        let out = s.up(IN_B);
        assert_eq!(
            out[1],
            (
                A,
                ControlEvent::DragEnd {
                    over: Some(B),
                    mods: Modifiers::NONE
                }
            )
        );
        // Hover catches up after release.
        assert_eq!(&out[2..], &[(B, ControlEvent::HoverEnter)]);

        s.down(IN_A);
        s.mv(BARE);
        let out = s.up(BARE);
        assert!(matches!(out[1].1, ControlEvent::DragEnd { over: None, .. }));
    }

    #[test]
    fn modifiers_sampled_per_drag_event() {
        let mut s = Script::new();
        s.down(IN_A);
        let out = s.mv(Point::new(25.0, 15.0));
        assert!(matches!(out[1].1, ControlEvent::Drag { mods, .. } if !mods.shift));
        let out = s.mv_with(Point::new(25.0, 10.0), Modifiers::SHIFT);
        assert!(matches!(out[0].1, ControlEvent::Drag { mods, .. } if mods.shift));
    }

    #[test]
    fn hover_enter_leave_across_controls() {
        let mut s = Script::new();
        let hover = |pos| ControlEvent::Hover { pos };
        assert_eq!(
            s.mv(IN_A),
            [(A, ControlEvent::HoverEnter), (A, hover(IN_A))]
        );
        let within = Point::new(30.0, 30.0);
        assert_eq!(s.mv(within), [(A, hover(within))]);
        assert_eq!(
            s.mv(IN_B),
            [
                (A, ControlEvent::HoverLeave),
                (B, ControlEvent::HoverEnter),
                (B, hover(IN_B))
            ]
        );
        assert_eq!(s.mv(BARE), [(B, ControlEvent::HoverLeave)]);
        s.mv(IN_A);
        assert_eq!(
            s.feed(RawKind::PointerLeave),
            [(A, ControlEvent::HoverLeave)]
        );
    }

    #[test]
    fn dwell_fires_once_after_stillness() {
        let mut s = Script::new();
        s.mv(IN_A);
        assert!(s.tick(499.0).is_empty());
        assert_eq!(s.tick(500.0), [(A, ControlEvent::HoverDwell)]);
        assert!(s.tick(2000.0).is_empty());
    }

    #[test]
    fn movement_rearms_dwell() {
        let mut s = Script::new();
        s.mv(IN_A);
        s.at(400.0).mv(Point::new(26.0, 25.0));
        assert!(s.tick(800.0).is_empty());
        assert_eq!(s.tick(900.0), [(A, ControlEvent::HoverDwell)]);
        // After firing, moving again re-arms.
        s.at(1000.0).mv(Point::new(27.0, 25.0));
        assert_eq!(s.tick(1500.0), [(A, ControlEvent::HoverDwell)]);
    }

    #[test]
    fn press_cancels_dwell() {
        let mut s = Script::new();
        s.mv(IN_A);
        s.at(100.0).down(IN_A);
        assert!(s.tick(1000.0).is_empty());
        s.at(1100.0).up(IN_A);
        assert!(s.tick(1500.0).is_empty(), "release alone doesn't re-arm");
    }

    #[test]
    fn wheel_goes_to_control_under_pointer() {
        let mut s = Script::new();
        let delta = Vec2::new(0.0, 1.0);
        let wheel = |pos| RawKind::Wheel {
            pos,
            delta,
            mods: Modifiers::NONE,
        };
        assert_eq!(
            s.feed(wheel(IN_B)),
            [(
                B,
                ControlEvent::Wheel {
                    delta,
                    mods: Modifiers::NONE
                }
            )]
        );
        assert!(s.feed(wheel(BARE)).is_empty());
    }

    #[test]
    fn keys_go_to_last_pressed_control() {
        let mut s = Script::new();
        let key = RawKind::Key {
            key: Key::ArrowUp,
            mods: Modifiers::NONE,
            pressed: true,
        };
        assert!(s.feed(key.clone()).is_empty(), "no focus yet");
        s.click(IN_B);
        let out = s.feed(key.clone());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, B);
        s.feed(RawKind::FocusLost);
        assert!(s.feed(key.clone()).is_empty());
        s.click(IN_A);
        s.click(BARE);
        assert!(
            s.feed(key).is_empty(),
            "pressing bare faceplate clears focus"
        );
    }

    #[test]
    fn drop_goes_to_control_under_pointer() {
        let mut s = Script::new();
        let payload = DropPayload::Paths(vec!["/tmp/x.wav".into()]);
        let out = s.feed(RawKind::Drop {
            pos: IN_A,
            payload: payload.clone(),
        });
        assert_eq!(
            out,
            [(
                A,
                ControlEvent::Drop {
                    payload: payload.clone()
                }
            )]
        );
        assert!(s.feed(RawKind::Drop { pos: BARE, payload }).is_empty());
    }

    #[test]
    fn press_on_bare_faceplate_emits_nothing() {
        let mut s = Script::new();
        let mut out = s.down(BARE);
        out.extend(s.mv(IN_A));
        out.extend(s.mv(IN_B));
        out.extend(s.up(IN_B));
        // Only the post-release hover catch-up.
        assert_eq!(out, [(B, ControlEvent::HoverEnter)]);
    }

    #[test]
    fn right_button_presses_but_never_drags_or_clicks() {
        let mut s = Script::new();
        let mut out = s.feed(RawKind::PointerDown {
            pos: IN_A,
            button: MouseButton::Right,
            mods: Modifiers::NONE,
        });
        out.extend(s.mv(Point::new(25.0, 0.0)));
        out.extend(s.feed(RawKind::PointerUp {
            pos: Point::new(25.0, 0.0),
            button: MouseButton::Right,
            mods: Modifiers::NONE,
        }));
        assert_eq!(kinds(&out), ["Press", "Release", "HoverEnter"]);
    }

    #[test]
    fn focus_lost_mid_drag_closes_the_drag() {
        let mut s = Script::new();
        s.down(IN_A);
        s.mv(Point::new(25.0, 10.0));
        let out = s.feed(RawKind::FocusLost);
        assert_eq!(kinds(&out), ["DragEnd"]);
        assert!(matches!(out[0].1, ControlEvent::DragEnd { over: None, .. }));
        // The stray release afterwards is ignored.
        assert!(kinds(&s.up(IN_A)).iter().all(|k| k.starts_with("Hover")));
    }

    #[test]
    fn cmd_click_carries_modifiers() {
        let mut s = Script::new();
        s.down_with(IN_A, Modifiers::CMD);
        let out = s.feed(RawKind::PointerUp {
            pos: IN_A,
            button: MouseButton::Left,
            mods: Modifiers::CMD,
        });
        assert_eq!(
            out[1].1,
            ControlEvent::Click {
                count: 1,
                mods: Modifiers::CMD
            }
        );
    }
}
