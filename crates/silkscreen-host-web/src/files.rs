//! Browser `File`s dropped on the editor, behind the `u32` handles that
//! [`DropPayload::Files`](silkscreen::core::DropPayload::Files) carries.

use std::cell::RefCell;
use std::collections::HashMap;

use web_sys::File;

thread_local! {
    static FILES: RefCell<Registry> = RefCell::new(Registry::default());
}

#[derive(Debug, Default)]
struct Registry {
    next: u32,
    files: HashMap<u32, File>,
}

/// Keep `files` from one drop and return their handles. The previous
/// drop's files that weren't taken are released: a drop's handles are
/// meant to be resolved while it is handled.
pub(crate) fn register(files: impl IntoIterator<Item = File>) -> Vec<u32> {
    FILES.with(|r| {
        let mut r = r.borrow_mut();
        r.files.clear();
        files
            .into_iter()
            .map(|file| {
                let handle = r.next;
                r.next = r.next.wrapping_add(1);
                r.files.insert(handle, file);
                handle
            })
            .collect()
    })
}

/// The dropped file behind `handle`, removed from the registry.
pub fn take_dropped_file(handle: u32) -> Option<File> {
    FILES.with(|r| r.borrow_mut().files.remove(&handle))
}
