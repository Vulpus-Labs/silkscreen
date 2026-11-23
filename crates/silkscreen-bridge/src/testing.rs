//! Test support (feature `testing`): counting the allocations a stretch
//! of code makes, so tests can hold engine-side code to design/engine-bridge.md's "no
//! allocation on the audio thread".
//!
//! A test binary installs the allocator itself, one line:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: silkscreen_bridge::testing::CountingAllocator =
//!     silkscreen_bridge::testing::CountingAllocator;
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting allocations on a thread while
/// [`with_allocations`] runs there.
#[derive(Debug, Clone, Copy, Default)]
pub struct CountingAllocator;

// Implementing `GlobalAlloc` is inherently unsafe; this forwards to the
// system allocator unchanged.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Run `f`, and how many allocations it made on this thread. Counts
/// only where [`CountingAllocator`] is the global allocator.
pub fn with_allocations<R>(f: impl FnOnce() -> R) -> (R, usize) {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    let out = f();
    COUNTING.with(|c| c.set(false));
    (out, COUNT.with(Cell::get))
}

/// How many allocations `f` made on this thread.
pub fn allocations(f: impl FnOnce()) -> usize {
    with_allocations(f).1
}
