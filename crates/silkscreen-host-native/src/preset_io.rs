//! Export and Import in native editors (design/presets.md): the
//! platform's Save and Open panels, through `rfd`'s async dialogs.
//!
//! A dialog is parented to the editor's window. On macOS that makes it a
//! sheet begun with a completion handler, so no nested modal run loop
//! re-enters the editor from inside a DAW callback; Windows and Linux run
//! the dialog on rfd's own thread. The view polls the dialog's future
//! each tick (from [`take_event`](PresetIo::take_event)) with a no-op
//! waker, and the file is written or read when the dialog answers. A
//! cancelled dialog reports nothing. One dialog is open at a time.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use baseview::WindowContext;
use rfd::{AsyncFileDialog, FileHandle};
use silkscreen::{IoEvent, PresetIo};
use silkscreen::{library, presets};

type Dialog = Pin<Box<dyn Future<Output = Option<FileHandle>>>>;

/// An open dialog, and what to do with the file it gives.
enum Pending {
    Export { dialog: Dialog, text: String },
    Import(Dialog),
}

/// Export and Import through native file dialogs over an editor window.
pub struct NativePresetIo {
    parent: WindowContext,
    pending: RefCell<Option<Pending>>,
    events: RefCell<VecDeque<IoEvent>>,
}

impl std::fmt::Debug for NativePresetIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePresetIo")
            .field("open", &self.pending.borrow().is_some())
            .field("events", &self.events.borrow().len())
            .finish_non_exhaustive()
    }
}

impl NativePresetIo {
    /// Dialogs over the window `parent` belongs to.
    pub fn new(parent: WindowContext) -> Self {
        Self {
            parent,
            pending: RefCell::new(None),
            events: RefCell::new(VecDeque::new()),
        }
    }

    fn dialog(&self, title: &str) -> AsyncFileDialog {
        AsyncFileDialog::new()
            .set_title(title)
            .set_parent(&self.parent)
    }

    /// Open `pending`'s dialog unless one is open already.
    fn open(&self, make: impl FnOnce() -> Pending) {
        let mut pending = self.pending.borrow_mut();
        if pending.is_none() {
            *pending = Some(make());
        }
    }

    /// Finish the open dialog if it has answered.
    fn poll(&self) {
        let mut slot = self.pending.borrow_mut();
        let Some(pending) = slot.as_mut() else {
            return;
        };
        let dialog = match pending {
            Pending::Export { dialog, .. } | Pending::Import(dialog) => dialog,
        };
        let mut cx = Context::from_waker(Waker::noop());
        let Poll::Ready(file) = dialog.as_mut().poll(&mut cx) else {
            return;
        };
        let pending = slot.take().expect("an open dialog");
        let Some(file) = file else {
            return;
        };
        let event = match pending {
            Pending::Export { text, .. } => write(file.path(), &text),
            Pending::Import(_) => match std::fs::read_to_string(file.path()) {
                Ok(text) => IoEvent::Imported(text),
                Err(e) => IoEvent::Failed(presets::import_failed(e)),
            },
        };
        self.events.borrow_mut().push_back(event);
    }
}

fn write(path: &Path, text: &str) -> IoEvent {
    match std::fs::write(path, text) {
        Ok(()) => IoEvent::Exported(path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )),
        Err(e) => IoEvent::Failed(presets::export_failed(e)),
    }
}

impl PresetIo for NativePresetIo {
    fn export(&self, file_name: &str, text: &str) {
        let is_library = Path::new(file_name)
            .extension()
            .is_some_and(|x| x == library::EXT);
        let (title, kind, ext) = if is_library {
            ("Export User Presets", "Preset library", library::EXT)
        } else {
            ("Export Preset", "Preset", presets::EXT)
        };
        let dialog = self
            .dialog(title)
            .set_file_name(file_name)
            .add_filter(kind, &[ext]);
        self.open(|| Pending::Export {
            dialog: Box::pin(dialog.save_file()),
            text: text.to_owned(),
        });
    }

    fn import(&self) {
        let dialog = self
            .dialog("Import Presets")
            .add_filter("Preset or library", &[presets::EXT, library::EXT]);
        self.open(|| Pending::Import(Box::pin(dialog.pick_file())));
    }

    fn take_event(&self) -> Option<IoEvent> {
        self.poll();
        self.events.borrow_mut().pop_front()
    }
}
