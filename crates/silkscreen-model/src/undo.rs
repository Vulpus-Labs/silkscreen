//! Undo history: one entry per completed user gesture ([design/model.md]).
//!
//! [design/model.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/model.md

use std::collections::VecDeque;

use crate::param::ParamIndex;

/// One completed gesture: `param` went from `before` to `after`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UndoEntry {
    pub param: ParamIndex,
    pub before: f32,
    pub after: f32,
}

/// One undo step: the gestures it reverts, in the order they happened.
/// Usually one; a group (e.g. loading a preset) has several.
pub(crate) type Step = Vec<UndoEntry>;

#[derive(Debug)]
pub(crate) struct UndoStack {
    done: VecDeque<Step>,
    undone: Vec<Step>,
    depth: usize,
    /// Gestures collected into one step while a group is open.
    group: Option<Step>,
}

impl UndoStack {
    pub const DEFAULT_DEPTH: usize = 100;

    pub fn new() -> Self {
        Self {
            done: VecDeque::new(),
            undone: Vec::new(),
            depth: Self::DEFAULT_DEPTH,
            group: None,
        }
    }

    /// Collect the gestures recorded until [`end_group`](Self::end_group)
    /// into one step. Groups don't nest; opening one while open is a no-op.
    pub fn begin_group(&mut self) {
        self.group.get_or_insert_with(Vec::new);
    }

    /// Close the group, recording it as one step if it holds anything.
    pub fn end_group(&mut self) {
        if let Some(step) = self.group.take()
            && !step.is_empty()
        {
            self.push(step);
        }
    }

    /// Record a new user edit. Clears the redo stack; drops the oldest
    /// entry beyond the depth limit.
    pub fn record(&mut self, entry: UndoEntry) {
        match &mut self.group {
            Some(step) => step.push(entry),
            None => self.push(vec![entry]),
        }
    }

    fn push(&mut self, step: Step) {
        self.undone.clear();
        self.done.push_back(step);
        self.trim();
    }

    pub fn pop_undo(&mut self) -> Option<Step> {
        let step = self.done.pop_back()?;
        self.undone.push(step.clone());
        Some(step)
    }

    pub fn pop_redo(&mut self) -> Option<Step> {
        let step = self.undone.pop()?;
        self.done.push_back(step.clone());
        Some(step)
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
        self.trim();
    }

    fn trim(&mut self) {
        while self.done.len() > self.depth {
            self.done.pop_front();
        }
    }
}
