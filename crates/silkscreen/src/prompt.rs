//! Names typed in a host's own window (design/input.md).
//!
//! A DAW takes keys it uses itself (Space, above all) before a plugin's
//! view sees them, so a name field drawn on the faceplate can't be typed
//! into reliably. A host that knows this gives the editor a
//! [`TextPrompt`]: name fields then ask it for their text, and it shows a
//! small window of its own that has the keyboard to itself. Hosts without
//! one (the web, the standalone app) keep typing into the field.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// What a prompt was answered with: `Some(text)` on Enter, `None` when it
/// was cancelled (Esc, a click elsewhere, the window closing).
pub type Answer = Option<String>;

type Slot = RefCell<Option<Answer>>;

/// A host's text prompt: a small window over the editor's with a title,
/// a field holding `initial` (all of it selected) and the keyboard.
pub trait TextPrompt {
    /// Show a prompt. Its answer arrives later, through the [`Prompt`]'s
    /// [`PromptReply`]; dropping the [`Prompt`] unanswered closes it.
    fn prompt(&self, title: &str, initial: &str) -> Prompt;
}

/// The editor's end of an open prompt.
pub struct Prompt {
    slot: Rc<Slot>,
    /// Closes the host's window; taken once the prompt has answered.
    dismiss: Option<Box<dyn FnOnce()>>,
}

impl std::fmt::Debug for Prompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prompt")
            .field("answer", &self.slot.borrow())
            .field("open", &self.dismiss.is_some())
            .finish()
    }
}

/// The host's end of an open prompt: how it answers.
#[derive(Clone, Debug)]
pub struct PromptReply(Weak<Slot>);

impl Prompt {
    /// A prompt the host closes with `dismiss` if the editor drops it
    /// unanswered, and the reply the host answers it with.
    pub fn new(dismiss: impl FnOnce() + 'static) -> (Self, PromptReply) {
        let slot = Rc::new(RefCell::new(None));
        let reply = PromptReply(Rc::downgrade(&slot));
        (
            Self {
                slot,
                dismiss: Some(Box::new(dismiss)),
            },
            reply,
        )
    }

    /// The answer, once, when it has come.
    pub fn answer(&mut self) -> Option<Answer> {
        let answer = self.slot.borrow_mut().take()?;
        // The host's window has gone with its answer.
        self.dismiss = None;
        Some(answer)
    }
}

impl Drop for Prompt {
    fn drop(&mut self) {
        if let Some(dismiss) = self.dismiss.take() {
            dismiss();
        }
    }
}

impl PromptReply {
    /// Answer the prompt. Later answers, and answers to a prompt the
    /// editor has dropped, are ignored.
    pub fn send(&self, answer: Answer) {
        if let Some(slot) = self.0.upgrade() {
            slot.borrow_mut().get_or_insert(answer);
        }
    }
}

/// Prompts a test answers by hand: the last one opened is answered with
/// [`answer`](Self::answer).
#[derive(Debug, Default)]
pub struct ScriptedPrompt {
    open: RefCell<Vec<(String, String, PromptReply)>>,
    dismissed: Rc<RefCell<usize>>,
}

impl ScriptedPrompt {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The title and initial text of every prompt opened so far.
    pub fn asked(&self) -> Vec<(String, String)> {
        self.open
            .borrow()
            .iter()
            .map(|(t, i, _)| (t.clone(), i.clone()))
            .collect()
    }

    /// Answer the last prompt opened.
    pub fn answer(&self, answer: Option<&str>) {
        let open = self.open.borrow();
        let (_, _, reply) = open.last().expect("a prompt is open");
        reply.send(answer.map(str::to_owned));
    }

    /// How many prompts the editor closed unanswered.
    pub fn dismissed(&self) -> usize {
        *self.dismissed.borrow()
    }
}

impl TextPrompt for ScriptedPrompt {
    fn prompt(&self, title: &str, initial: &str) -> Prompt {
        let dismissed = Rc::clone(&self.dismissed);
        let (prompt, reply) = Prompt::new(move || *dismissed.borrow_mut() += 1);
        self.open
            .borrow_mut()
            .push((title.to_owned(), initial.to_owned(), reply));
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_arrives_once_and_needs_no_dismissal() {
        let host = ScriptedPrompt::new();
        let mut p = host.prompt("Name", "Init");
        assert_eq!(p.answer(), None);
        host.answer(Some("Pad"));
        host.answer(Some("ignored"));
        assert_eq!(p.answer(), Some(Some("Pad".into())));
        assert_eq!(p.answer(), None);
        drop(p);
        assert_eq!(host.dismissed(), 0);
    }

    #[test]
    fn dropping_an_open_prompt_dismisses_it() {
        let host = ScriptedPrompt::new();
        drop(host.prompt("Name", ""));
        assert_eq!(host.dismissed(), 1);
        // A late answer goes nowhere.
        host.answer(None);
    }
}
