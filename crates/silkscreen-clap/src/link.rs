//! The plugin's end of the bridge, and the UI → host event path.

use clack_plugin::events::event_types::{
    ParamGestureBeginEvent, ParamGestureEndEvent, ParamValueEvent,
};
use clack_plugin::prelude::*;
use clack_plugin::utils::Cookie;
use silkscreen::ParamSpec;
use silkscreen::bridge::{EngineEnd, EngineEvent};

/// Page groups whose shown page a [`Link`] remembers.
pub(crate) const SHOWN_GROUPS: usize = 8;

/// The engine end of the bridge, which host gestures are open, and the
/// page each group last showed. Owned by the main thread while inactive
/// and by the processor while active, so all travel together.
#[derive(Debug)]
pub(crate) struct Link {
    pub end: EngineEnd,
    open: Box<[bool]>,
    /// Each group's last shown page, for an engine made after the editor
    /// said so: one made at a re-activation, or one activated after a
    /// `Shown` the inactive plugin drained with no engine to tell.
    shown: [Option<u16>; SHOWN_GROUPS],
}

impl Link {
    pub fn new(end: EngineEnd) -> Self {
        let open = vec![false; end.param_count()].into_boxed_slice();
        Self {
            end,
            open,
            shown: [None; SHOWN_GROUPS],
        }
    }

    /// Tell `engine` each group's last shown page: what a new engine needs
    /// at activation, since the editor only says when its page changes.
    pub fn replay_shown(&self, mut engine: impl FnMut(u16, u16)) {
        for (group, page) in self.shown.iter().enumerate() {
            if let Some(page) = *page {
                engine(group as u16, page);
            }
        }
    }

    /// Drain the UI's events: values go to `to_engine` and to the host as
    /// param-value events; gestures go to the host as gesture begin/end,
    /// balanced; the shown page goes to `to_engine` only, and is
    /// remembered for [`replay_shown`](Self::replay_shown). A pending state
    /// load goes first, to `to_engine` only, as a patch load and then values:
    /// the host chose that state, so it isn't told of it as edits. A patch
    /// load from the UI goes to `to_engine` only.
    /// After a load or a resync, any gesture still open is closed, since
    /// its end may have been lost. Every output event is at time 0. Never
    /// allocates.
    pub fn ui_to_host(
        &mut self,
        specs: &[ParamSpec],
        output: &mut OutputEvents,
        mut to_engine: impl FnMut(EngineEvent),
    ) {
        let (open, shown) = (&mut self.open, &mut self.shown);
        let loaded = self
            .end
            .take_load(silkscreen::bridge::patch_loaded(&mut to_engine));
        let resynced = self.end.drain(|event| match event {
            EngineEvent::PatchLoad => to_engine(event),
            EngineEvent::Shown { group, page } => {
                if let Some(slot) = shown.get_mut(usize::from(group)) {
                    *slot = Some(page);
                }
                to_engine(event);
            }
            EngineEvent::ParamValue { index, value } => {
                to_engine(event);
                let id = ClapId::new(specs[index.index()].id);
                let _ = output.try_push(ParamValueEvent::new(
                    0,
                    id,
                    Pckn::match_all(),
                    f64::from(value),
                    Cookie::empty(),
                ));
            }
            EngineEvent::GestureBegin { index } => {
                if !std::mem::replace(&mut open[index.index()], true) {
                    let _ = output.try_push(ParamGestureBeginEvent::new(
                        0,
                        ClapId::new(specs[index.index()].id),
                    ));
                }
            }
            EngineEvent::GestureEnd { index } => {
                if std::mem::replace(&mut open[index.index()], false) {
                    let _ = output.try_push(ParamGestureEndEvent::new(
                        0,
                        ClapId::new(specs[index.index()].id),
                    ));
                }
            }
        });
        if loaded || resynced {
            for (i, o) in open.iter_mut().enumerate() {
                if std::mem::replace(o, false) {
                    let _ = output.try_push(ParamGestureEndEvent::new(0, ClapId::new(specs[i].id)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clack_plugin::events::io::EventBuffer;
    use silkscreen::bridge::bridge;
    use silkscreen::{EngineLink, ParamIndex};

    static SPECS: [ParamSpec; 2] = [
        ParamSpec::new("A", 0.0, 1.0, 0.0).id(100),
        ParamSpec::new("B", 0.0, 1.0, 0.0).id(200),
    ];
    const A: ParamIndex = ParamIndex(0);
    const B: ParamIndex = ParamIndex(1);

    /// Each output event as a short string: "B100", "V100=0.5", "E100".
    ///
    /// Gestures are decoded with `as_event`: at the pinned clack rev,
    /// `CoreEventSpace::from_unknown` doesn't recognise gesture events
    /// (the enum has the variants, the decoder lacks the arms). Hosts read
    /// the raw CLAP structs, so this only affects decoding on our side.
    fn shape(buf: &EventBuffer) -> Vec<String> {
        buf.iter()
            .map(|e| {
                if let Some(g) = e.as_event::<ParamGestureBeginEvent>() {
                    format!("B{}", g.param_id().unwrap().get())
                } else if let Some(g) = e.as_event::<ParamGestureEndEvent>() {
                    format!("E{}", g.param_id().unwrap().get())
                } else if let Some(v) = e.as_event::<ParamValueEvent>() {
                    format!("V{}={}", v.param_id().unwrap().get(), v.value())
                } else {
                    panic!("unexpected {e:?}")
                }
            })
            .collect()
    }

    fn run(link: &mut Link, applied: &mut Vec<(ParamIndex, f32)>) -> Vec<String> {
        run_all(link, &mut |e| {
            if let EngineEvent::ParamValue { index, value } = e {
                applied.push((index, value));
            }
        })
    }

    /// [`run`], with every event to the engine.
    fn run_all(link: &mut Link, to_engine: &mut dyn FnMut(EngineEvent)) -> Vec<String> {
        let mut out = EventBuffer::new();
        link.ui_to_host(&SPECS, &mut out.as_output(), to_engine);
        shape(&out)
    }

    /// A state load reaches the engine marked as a patch load, ahead
    /// of its values; a preset's mark from the UI goes to the engine only.
    #[test]
    fn a_patch_load_reaches_the_engine_first_and_not_the_host() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.load(&[0.25, 0.5]);
        let mut seen = Vec::new();
        assert!(run_all(&mut link, &mut |e| seen.push(e)).is_empty());
        assert_eq!(
            seen,
            [
                EngineEvent::PatchLoad,
                EngineEvent::ParamValue {
                    index: A,
                    value: 0.25
                },
                EngineEvent::ParamValue {
                    index: B,
                    value: 0.5
                },
            ]
        );

        ui.send(EngineEvent::PatchLoad);
        seen.clear();
        assert!(run_all(&mut link, &mut |e| seen.push(e)).is_empty());
        assert_eq!(seen, [EngineEvent::PatchLoad]);
    }

    #[test]
    fn ui_gestures_become_host_events_with_stable_ids() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.send(EngineEvent::GestureBegin { index: B });
        ui.send(EngineEvent::ParamValue {
            index: B,
            value: 0.25,
        });
        ui.send(EngineEvent::ParamValue {
            index: B,
            value: 0.5,
        });
        ui.send(EngineEvent::GestureEnd { index: B });
        let mut applied = Vec::new();
        assert_eq!(
            run(&mut link, &mut applied),
            ["B200", "V200=0.25", "V200=0.5", "E200"]
        );
        assert_eq!(applied, [(B, 0.25), (B, 0.5)]);
    }

    #[test]
    fn gestures_are_balanced() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.send(EngineEvent::GestureEnd { index: A }); // stray end: dropped
        ui.send(EngineEvent::GestureBegin { index: A });
        ui.send(EngineEvent::GestureBegin { index: A }); // repeat: dropped
        let mut applied = Vec::new();
        assert_eq!(run(&mut link, &mut applied), ["B100"]);
        ui.send(EngineEvent::GestureEnd { index: A });
        assert_eq!(run(&mut link, &mut applied), ["E100"]);
    }

    #[test]
    fn a_load_reaches_the_engine_but_not_the_host() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.send(EngineEvent::GestureBegin { index: A });
        let mut applied = Vec::new();
        assert_eq!(run(&mut link, &mut applied), ["B100"]);

        // The load supersedes the queued value and the gesture's end.
        ui.send(EngineEvent::ParamValue {
            index: A,
            value: 0.9,
        });
        ui.send(EngineEvent::GestureEnd { index: A });
        ui.load(&[0.25, 0.5]);
        assert_eq!(run(&mut link, &mut applied), ["E100"], "only the close");
        assert_eq!(applied, [(A, 0.25), (B, 0.5)]);
    }

    #[test]
    fn a_resync_closes_open_gestures() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 4);
        let mut link = Link::new(end);
        ui.send(EngineEvent::GestureBegin { index: A });
        let mut applied = Vec::new();
        assert_eq!(run(&mut link, &mut applied), ["B100"]);

        // Overflow: the end of A's gesture is among the discarded events.
        for i in 0..8 {
            ui.send(EngineEvent::ParamValue {
                index: A,
                value: i as f32 / 10.0,
            });
        }
        ui.send(EngineEvent::GestureEnd { index: A });
        let shape = run(&mut link, &mut applied);
        assert_eq!(shape.last().map(String::as_str), Some("E100"), "{shape:?}");
        assert!(
            shape.contains(&"V100=0.699999988079071".to_string()),
            "{shape:?}"
        );
        // Nothing left open, so nothing more is closed.
        assert!(run(&mut link, &mut applied).is_empty());
    }

    #[test]
    fn the_shown_page_goes_to_the_engine_not_the_host() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.send(EngineEvent::Shown { group: 0, page: 1 });
        let mut out = EventBuffer::new();
        let mut seen = Vec::new();
        link.ui_to_host(&SPECS, &mut out.as_output(), |e| seen.push(e));
        assert_eq!(seen, [EngineEvent::Shown { group: 0, page: 1 }]);
        assert!(shape(&out).is_empty(), "the host isn't told");
    }

    /// A new engine (a re-activation, or a first activation after the
    /// inactive plugin drained the editor's `Shown`) is told each group's
    /// last page, not left at its own default.
    #[test]
    fn the_last_shown_page_replays_to_a_new_engine() {
        let (mut ui, end) = bridge(&[0.0, 0.0], 64);
        let mut link = Link::new(end);
        ui.send(EngineEvent::Shown { group: 0, page: 1 });
        ui.send(EngineEvent::Shown { group: 2, page: 4 });
        ui.send(EngineEvent::Shown { group: 0, page: 2 });
        ui.send(EngineEvent::Shown {
            group: SHOWN_GROUPS as u16,
            page: 9,
        });
        let mut out = EventBuffer::new();
        // Inactive: drained with no engine to tell.
        link.ui_to_host(&SPECS, &mut out.as_output(), |_| {});
        let mut replayed = Vec::new();
        link.replay_shown(|group, page| replayed.push((group, page)));
        assert_eq!(
            replayed,
            [(0, 2), (2, 4)],
            "groups past the table aren't kept"
        );
    }
}
