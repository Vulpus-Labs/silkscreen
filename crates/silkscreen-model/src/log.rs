//! The model's append-only event log, read independently by each consumer.

use std::collections::VecDeque;

use crate::ParamIndex;

/// Who caused a change ([design/model.md]).
///
/// [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The user, through the editor.
    Ui,
    /// The host: automation, its generic param UI, state restore. The engine
    /// already has these values, so they are not sent back to it.
    Host,
    /// An undo or redo step.
    Undo,
}

/// "This event occurred": what the model publishes to the view and engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModelEvent {
    ParamChanged {
        param: ParamIndex,
        value: f32,
        origin: Origin,
    },
    GestureBegin {
        param: ParamIndex,
        origin: Origin,
    },
    GestureEnd {
        param: ParamIndex,
        origin: Origin,
    },
}

impl ModelEvent {
    pub fn param(&self) -> ParamIndex {
        match *self {
            ModelEvent::ParamChanged { param, .. }
            | ModelEvent::GestureBegin { param, .. }
            | ModelEvent::GestureEnd { param, .. } => param,
        }
    }

    pub fn origin(&self) -> Origin {
        match *self {
            ModelEvent::ParamChanged { origin, .. }
            | ModelEvent::GestureBegin { origin, .. }
            | ModelEvent::GestureEnd { origin, .. } => origin,
        }
    }
}

/// A consumer's read position in an [`EventLog`]: the sequence number of the
/// next event it will see.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor(u64);

/// Append-only, sequence-numbered event log. Each consumer holds its own
/// [`Cursor`]; [`compact`](Self::compact) drops what every cursor has read.
#[derive(Debug, Default)]
pub struct EventLog {
    /// Sequence number of `events[0]`.
    base: u64,
    events: VecDeque<ModelEvent>,
}

impl EventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sequence number the next appended event will get.
    pub fn end(&self) -> u64 {
        self.base + self.events.len() as u64
    }

    /// A cursor that will see only events appended from now on.
    pub fn cursor(&self) -> Cursor {
        Cursor(self.end())
    }

    pub(crate) fn push(&mut self, event: ModelEvent) {
        self.events.push_back(event);
    }

    /// Events appended since `cursor` was last read. Advances the cursor to
    /// the end immediately, whether or not the iterator is consumed.
    pub fn read(&self, cursor: &mut Cursor) -> impl ExactSizeIterator<Item = &ModelEvent> {
        debug_assert!(
            cursor.0 >= self.base,
            "cursor {} was compacted away (log base {})",
            cursor.0,
            self.base
        );
        let start = (cursor.0.saturating_sub(self.base) as usize).min(self.events.len());
        cursor.0 = self.end();
        self.events.range(start..)
    }

    /// Drop events every given cursor has read.
    pub fn compact(&mut self, cursors: &[Cursor]) {
        let Some(min) = cursors.iter().map(|c| c.0).min() else {
            return;
        };
        let drop = (min.saturating_sub(self.base) as usize).min(self.events.len());
        self.events.drain(..drop);
        self.base += drop as u64;
    }

    /// Number of events currently retained.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(i: u32) -> ModelEvent {
        ModelEvent::GestureBegin {
            param: ParamIndex(i),
            origin: Origin::Ui,
        }
    }

    fn params<'a>(it: impl Iterator<Item = &'a ModelEvent>) -> Vec<u32> {
        it.map(|e| e.param().0).collect()
    }

    #[test]
    fn cursors_read_independently() {
        let mut log = EventLog::new();
        let mut fast = log.cursor();
        let mut slow = log.cursor();
        log.push(ev(1));
        log.push(ev(2));
        assert_eq!(params(log.read(&mut fast)), [1, 2]);
        log.push(ev(3));
        assert_eq!(params(log.read(&mut fast)), [3]);
        assert_eq!(params(log.read(&mut fast)), Vec::<u32>::new());
        assert_eq!(params(log.read(&mut slow)), [1, 2, 3]);
    }

    #[test]
    fn new_cursor_sees_only_future_events() {
        let mut log = EventLog::new();
        log.push(ev(1));
        let mut c = log.cursor();
        log.push(ev(2));
        assert_eq!(params(log.read(&mut c)), [2]);
    }

    #[test]
    fn compact_keeps_what_the_slowest_cursor_needs() {
        let mut log = EventLog::new();
        let mut a = log.cursor();
        let mut b = log.cursor();
        for i in 0..5 {
            log.push(ev(i));
        }
        let _ = log.read(&mut a);
        log.compact(&[a, b]);
        assert_eq!(log.len(), 5, "b has read nothing");

        let _ = log.read(&mut b);
        log.push(ev(5));
        log.compact(&[a, b]);
        assert_eq!(log.len(), 1);
        assert_eq!(params(log.read(&mut a)), [5]);
        assert_eq!(params(log.read(&mut b)), [5]);
        log.compact(&[a, b]);
        assert!(log.is_empty());
        assert_eq!(log.end(), 6, "sequence numbers survive compaction");
    }
}
