//! The standalone app's menu bar: an application menu with Quit,
//! and an Edit menu whose Undo and Redo reach the editor.
//!
//! Menu actions arrive on the main thread, outside the window's handler,
//! so they queue a [`MenuCommand`]; the window applies the queue on its
//! next timer tick. macOS only: elsewhere there's no menu, and nothing is
//! ever queued.

use std::cell::RefCell;

/// An editing command from the menu bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum MenuCommand {
    Undo,
    Redo,
}

thread_local! {
    static QUEUE: RefCell<Vec<MenuCommand>> = const { RefCell::new(Vec::new()) };
}

/// Queue `command` for the window (main thread).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn push(command: MenuCommand) {
    QUEUE.with(|q| q.borrow_mut().push(command));
}

/// The commands queued since the last call, in order.
pub(crate) fn take() -> Vec<MenuCommand> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

/// Install the menu bar for an app called `name`. Call on the main thread
/// once the application exists (inside the window builder). A no-op off
/// macOS.
#[cfg(not(target_os = "macos"))]
pub(crate) fn install(_name: &str) {}

#[cfg(target_os = "macos")]
pub(crate) use macos::install;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;

    use super::{MenuCommand, push};

    define_class!(
        // The menu items' target. One app, one window: a fixed class name
        // can't collide here as it would between two plugins.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SilkscreenEditMenuTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(silkscreenUndo:))]
            fn undo(&self, _sender: Option<&AnyObject>) {
                push(MenuCommand::Undo);
            }

            #[unsafe(method(silkscreenRedo:))]
            fn redo(&self, _sender: Option<&AnyObject>) {
                push(MenuCommand::Redo);
            }
        }
    );

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            // SAFETY: NSObject's `init`, on a class that adds no state.
            unsafe { msg_send![Self::alloc(mtm), init] }
        }
    }

    thread_local! {
        /// Items hold their target weakly; this keeps it alive.
        static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
    }

    fn item(
        mtm: MainThreadMarker,
        title: &str,
        action: Sel,
        key: &str,
        modifiers: NSEventModifierFlags,
    ) -> Retained<NSMenuItem> {
        // SAFETY: `action` is a selector the item's target (or, for nil
        // targets, the responder chain) implements.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        item.setKeyEquivalentModifierMask(modifiers);
        item
    }

    fn submenu(
        mtm: MainThreadMarker,
        title: &str,
        items: &[Retained<NSMenuItem>],
    ) -> Retained<NSMenuItem> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for i in items {
            menu.addItem(i);
        }
        let holder = NSMenuItem::new(mtm);
        holder.setSubmenu(Some(&menu));
        holder
    }

    pub(crate) fn install(name: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let target = Target::new(mtm);
        let command = NSEventModifierFlags::Command;
        let undo = item(mtm, "Undo", sel!(silkscreenUndo:), "z", command);
        // An uppercase key equivalent implies Shift: ⇧⌘Z.
        let redo = item(mtm, "Redo", sel!(silkscreenRedo:), "Z", command);
        for i in [&undo, &redo] {
            // SAFETY: the target implements the item's action, and TARGET
            // keeps it alive for the app's life.
            unsafe { i.setTarget(Some(&target)) };
        }
        let quit = item(mtm, &format!("Quit {name}"), sel!(terminate:), "q", command);

        let bar = NSMenu::new(mtm);
        bar.addItem(&submenu(mtm, name, &[quit]));
        bar.addItem(&submenu(mtm, "Edit", &[undo, redo]));
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
        TARGET.with(|t| *t.borrow_mut() = Some(target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_queue_in_order_and_are_taken_once() {
        push(MenuCommand::Undo);
        push(MenuCommand::Undo);
        push(MenuCommand::Redo);
        assert_eq!(
            take(),
            [MenuCommand::Undo, MenuCommand::Undo, MenuCommand::Redo]
        );
        assert!(take().is_empty());
    }
}
