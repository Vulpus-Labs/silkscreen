//! Reading the system clipboard's text, for Paste and Cmd/Ctrl-V.
//! macOS reads the general pasteboard; Windows the Unicode text format.
//! Other platforms have none yet.

use std::rc::Rc;

use silkscreen::Clipboard;

/// The platform's clipboard, if this host can read it.
pub(crate) fn system() -> Option<Rc<dyn Clipboard>> {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        Some(Rc::new(System))
    } else {
        None
    }
}

#[derive(Debug)]
struct System;

impl Clipboard for System {
    fn text(&self) -> Option<String> {
        read().filter(|t| !t.is_empty())
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // AppKit FFI: a static string constant
fn read() -> Option<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    let board = NSPasteboard::generalPasteboard();
    // SAFETY: an immutable string constant AppKit exports.
    let kind = unsafe { NSPasteboardTypeString };
    board.stringForType(kind).map(|s| s.to_string())
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // Win32 clipboard FFI
fn read() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    let format = u32::from(CF_UNICODETEXT);
    // SAFETY: the clipboard is opened, read and closed on this thread;
    // the handle's memory is only read between GlobalLock and
    // GlobalUnlock, up to the NUL the format guarantees.
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 || OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let handle = GetClipboardData(format);
        let text = if handle.is_null() {
            None
        } else {
            let ptr = GlobalLock(handle).cast::<u16>();
            if ptr.is_null() {
                None
            } else {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                GlobalUnlock(handle);
                Some(text)
            }
        };
        CloseClipboard();
        text
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read() -> Option<String> {
    None
}
