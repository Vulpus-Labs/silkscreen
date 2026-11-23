//! Name prompts for editors embedded in a DAW.
//!
//! A DAW takes keys it uses itself (Space, above all) before a plugin's
//! view sees them, so names aren't typed into the faceplate there. The
//! editor asks the host instead ([`silkscreen::prompt`]), and the prompt
//! is a window of its own that has the keyboard: an alert sheet on macOS,
//! a small owned popup on Windows. Other platforms have none, and their
//! fields take keys as before.

use std::rc::Rc;

use raw_window_handle::HasWindowHandle;
use silkscreen::TextPrompt;

/// The platform's prompt over the editor window `handle`, if it has one.
pub(crate) fn for_window(handle: &impl HasWindowHandle) -> Option<Rc<dyn TextPrompt>> {
    #[cfg(target_os = "macos")]
    {
        mac::Sheet::new(handle).map(|p| Rc::new(p) as Rc<dyn TextPrompt>)
    }
    #[cfg(target_os = "windows")]
    {
        win::Popup::new(handle).map(|p| Rc::new(p) as Rc<dyn TextPrompt>)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = handle;
        None
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // AppKit FFI
mod mac {
    //! An `NSAlert` with a text field, run as a sheet on the DAW's window
    //! with a completion handler: no nested modal loop re-enters the
    //! editor from a DAW callback, and no Objective-C class is declared,
    //! so two plugins in one process can't collide. The sheet
    //! is its own key window, outside the DAW's handling of the editor's
    //! keys. Return presses OK, Esc presses Cancel.

    use std::cell::Cell;
    use std::rc::Rc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponse, NSTextField, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use silkscreen::{Prompt, TextPrompt};

    const FIELD_W: f64 = 260.0;
    const FIELD_H: f64 = 24.0;

    /// Prompts as sheets on the window holding the editor's view.
    #[derive(Debug)]
    pub(super) struct Sheet {
        view: Retained<NSView>,
        mtm: MainThreadMarker,
    }

    impl Sheet {
        pub(super) fn new(handle: &impl HasWindowHandle) -> Option<Self> {
            let RawWindowHandle::AppKit(h) = handle.window_handle().ok()?.as_raw() else {
                return None;
            };
            let mtm = MainThreadMarker::new()?;
            // SAFETY: an AppKit handle's `ns_view` is a live NSView;
            // retaining it keeps it so for as long as we hold it.
            let view = unsafe { Retained::retain(h.ns_view.as_ptr().cast::<NSView>()) }?;
            Some(Self { view, mtm })
        }
    }

    impl TextPrompt for Sheet {
        fn prompt(&self, title: &str, initial: &str) -> Prompt {
            let alert = NSAlert::new(self.mtm);
            alert.setMessageText(&NSString::from_str(title));
            alert.addButtonWithTitle(&NSString::from_str("OK"));
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FIELD_W, FIELD_H));
            let field = NSTextField::initWithFrame(NSTextField::alloc(self.mtm), frame);
            field.setStringValue(&NSString::from_str(initial));
            alert.setAccessoryView(Some(&field));
            alert.layout();
            // The field has the keyboard, its text all selected.
            alert.window().setInitialFirstResponder(Some(&field));

            let Some(parent) = self.view.window() else {
                // Not in a window (yet): nothing to show it on.
                let (prompt, reply) = Prompt::new(|| {});
                reply.send(None);
                return prompt;
            };
            let sheet = alert.window();
            let host = parent.clone();
            // Only a sheet that opened is closed: a prompt dropped while
            // `beginSheet` unwinds must not end a sheet that never began
            //.
            let begun = Rc::new(Cell::new(false));
            let opened = Rc::clone(&begun);
            let (prompt, reply) = Prompt::new(move || {
                if opened.get() {
                    host.endSheet(&sheet);
                }
            });
            let answer = field.clone();
            let handler = RcBlock::new(move |response: NSModalResponse| {
                let text = (response == NSAlertFirstButtonReturn)
                    .then(|| answer.stringValue().to_string());
                reply.send(text);
            });
            alert.beginSheetModalForWindow_completionHandler(&parent, Some(&handler));
            begun.set(true);
            prompt
        }
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code, unsafe_op_in_unsafe_fn)] // Win32 FFI, unsafe throughout
mod win {
    //! A topmost popup owned by the DAW's window, with an `EDIT` control
    //! that has the keyboard. Its
    //! subclassed window procedure takes Return and Esc before the DAW's
    //! accelerators can. Deactivating it (a click elsewhere) cancels.

    use std::cell::Cell;
    use std::ptr;
    use std::rc::Rc;
    use std::sync::Once;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use silkscreen::{Prompt, PromptReply, TextPrompt};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CallWindowProcW, CreateWindowExW, DLGC_WANTALLKEYS, DefWindowProcW,
        DestroyWindow, ES_AUTOHSCROLL, GA_ROOT, GWLP_USERDATA, GWLP_WNDPROC, GetAncestor,
        GetParent, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        IDC_ARROW, IsWindow, LoadCursorW, RegisterClassExW, SendMessageW, SetForegroundWindow,
        SetWindowLongPtrW, WA_ACTIVE, WA_CLICKACTIVE, WA_INACTIVE, WM_ACTIVATE, WM_GETDLGCODE,
        WM_KEYDOWN, WM_NCDESTROY, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CHILD, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    // Stable Win32 values from modules this crate doesn't otherwise enable.
    const EM_SETSEL: u32 = 0x00B1;
    const SS_LEFT: u32 = 0;

    const WIDTH: i32 = 320;
    const HEIGHT: i32 = 84;
    const PAD: i32 = 12;
    const TITLE_H: i32 = 16;
    const FIELD_H: i32 = 24;
    const GAP: i32 = 6;

    /// Each plugin DLL registers its own class: the name carries the
    /// address of something in this DLL, so two never collide.
    static REGISTER: Once = Once::new();

    fn class_name() -> Vec<u16> {
        wide(&format!(
            "SilkscreenPrompt{:x}",
            (&raw const REGISTER) as usize
        ))
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().filter(|&c| c != 0).chain([0]).collect()
    }

    /// One popup's state, in both windows' `GWLP_USERDATA`. The popup's
    /// `WM_NCDESTROY` frees it; `reply.take()` answers once.
    struct State {
        reply: Option<PromptReply>,
        edit_proc: isize,
        /// Set at the first activation, so the inactive flicker while it
        /// is built isn't taken for a click elsewhere.
        activated: bool,
    }

    /// Prompts over the DAW window holding the editor's.
    #[derive(Debug)]
    pub(super) struct Popup {
        hwnd: HWND,
    }

    impl Popup {
        pub(super) fn new(handle: &impl HasWindowHandle) -> Option<Self> {
            let RawWindowHandle::Win32(h) = handle.window_handle().ok()?.as_raw() else {
                return None;
            };
            Some(Self {
                hwnd: h.hwnd.get() as HWND,
            })
        }
    }

    impl TextPrompt for Popup {
        fn prompt(&self, title: &str, initial: &str) -> Prompt {
            let shown: Rc<Cell<HWND>> = Rc::new(Cell::new(ptr::null_mut()));
            let popup = Rc::clone(&shown);
            let (prompt, reply) = Prompt::new(move || {
                let popup = popup.get();
                // SAFETY: Win32 calls on a window we made, on the thread
                // that runs it; one already gone is skipped.
                unsafe {
                    if !popup.is_null() && IsWindow(popup) != 0 {
                        DestroyWindow(popup);
                    }
                }
            });
            // SAFETY: as above, for the popup and its children, owned by
            // the editor's top-level window.
            match unsafe { open(self.hwnd, title, initial, reply.clone()) } {
                Some(popup) => shown.set(popup),
                None => reply.send(None),
            }
            prompt
        }
    }

    unsafe fn open(editor: HWND, title: &str, initial: &str, reply: PromptReply) -> Option<HWND> {
        let instance = GetModuleHandleW(ptr::null());
        let class = class_name();
        REGISTER.call_once(|| {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(popup_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_BTNFACE as usize + 1) as HBRUSH,
                lpszMenuName: ptr::null(),
                lpszClassName: class.as_ptr(),
                hIconSm: ptr::null_mut(),
            };
            RegisterClassExW(&raw const wc);
        });

        let owner = GetAncestor(editor, GA_ROOT);
        let (x, y) = centred_over(owner);
        let title_w = wide(title);
        let popup = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title_w.as_ptr(),
            WS_POPUP | WS_BORDER | WS_VISIBLE,
            x,
            y,
            WIDTH,
            HEIGHT,
            owner,
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if popup.is_null() {
            return None;
        }
        let label = wide("STATIC");
        CreateWindowExW(
            0,
            label.as_ptr(),
            title_w.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            PAD,
            PAD,
            WIDTH - 2 * PAD,
            TITLE_H,
            popup,
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        let edit_class = wide("EDIT");
        let initial_w = wide(initial);
        let edit = CreateWindowExW(
            0,
            edit_class.as_ptr(),
            initial_w.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL as u32,
            PAD,
            PAD + TITLE_H + GAP,
            WIDTH - 2 * PAD,
            FIELD_H,
            popup,
            ptr::null_mut(),
            instance,
            ptr::null(),
        );

        let state = Box::into_raw(Box::new(State {
            reply: Some(reply),
            edit_proc: 0,
            activated: false,
        }));
        SetWindowLongPtrW(popup, GWLP_USERDATA, state as isize);
        (*state).edit_proc = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize);
        SetWindowLongPtrW(edit, GWLP_USERDATA, state as isize);

        SendMessageW(edit, EM_SETSEL, 0, -1);
        SetFocus(edit);
        SetForegroundWindow(popup);
        Some(popup)
    }

    unsafe fn centred_over(owner: HWND) -> (i32, i32) {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if !owner.is_null() && GetWindowRect(owner, &raw mut r) != 0 {
            (
                i32::midpoint(r.left, r.right) - WIDTH / 2,
                i32::midpoint(r.top, r.bottom) - HEIGHT / 2,
            )
        } else {
            (100, 100)
        }
    }

    /// Answer and close `popup`, once.
    unsafe fn finish(popup: HWND, state: *mut State, answer: Option<String>) {
        if state.is_null() {
            return;
        }
        if let Some(reply) = (*state).reply.take() {
            reply.send(answer);
            DestroyWindow(popup);
        }
    }

    unsafe extern "system" fn popup_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        match msg {
            WM_ACTIVATE if !state.is_null() => {
                let how = (wparam as u32) & 0xFFFF;
                if how == WA_ACTIVE || how == WA_CLICKACTIVE {
                    (*state).activated = true;
                } else if how == WA_INACTIVE && (*state).activated {
                    finish(hwnd, state, None);
                }
                0
            }
            WM_NCDESTROY => {
                if !state.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let mut state = Box::from_raw(state);
                    // Gone without an answer (the editor closed it, or
                    // the DAW tore it down): a cancel.
                    if let Some(reply) = state.reply.take() {
                        reply.send(None);
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe extern "system" fn edit_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if msg == WM_GETDLGCODE {
            return DLGC_WANTALLKEYS as LRESULT;
        }
        if msg == WM_KEYDOWN {
            let popup = GetParent(hwnd);
            if wparam as u32 == u32::from(VK_RETURN) {
                finish(popup, state, Some(text(hwnd)));
                return 0;
            }
            if wparam as u32 == u32::from(VK_ESCAPE) {
                finish(popup, state, None);
                return 0;
            }
        }
        let original = if state.is_null() {
            0
        } else {
            (*state).edit_proc
        };
        if original == 0 {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let original: WNDPROC = std::mem::transmute(original);
        CallWindowProcW(original, hwnd, msg, wparam, lparam)
    }

    unsafe fn text(edit: HWND) -> String {
        let n = GetWindowTextLengthW(edit);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let read = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..read.max(0) as usize])
    }
}
