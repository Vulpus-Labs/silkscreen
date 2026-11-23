//! Shared 32-bit words: the storage under the web transport ([`crate::sab`])
//! and telemetry ([`crate::telemetry`]), so the same code runs over native
//! atomics and a `SharedArrayBuffer`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Shared 32-bit words, each accessed atomically. Every operation is at
/// least acquire (loads) or release (stores); read-modify-writes are both.
pub trait SharedWords {
    fn len(&self) -> usize;
    fn load(&self, i: usize) -> u32;
    fn store(&self, i: usize, value: u32);
    fn swap(&self, i: usize, value: u32) -> u32;
    fn fetch_or(&self, i: usize, value: u32) -> u32;
    fn fetch_add(&self, i: usize, value: u32) -> u32;
    /// Store `new` if the word holds `current`; the word's value before,
    /// as `Ok` if it was `current`.
    fn compare_exchange(&self, i: usize, current: u32, new: u32) -> Result<u32, u32>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Raise the word to `value` if that's larger; the value before.
    fn fetch_max(&self, i: usize, value: u32) -> u32 {
        let mut seen = self.load(i);
        while seen < value {
            match self.compare_exchange(i, seen, value) {
                Ok(_) => break,
                Err(now) => seen = now,
            }
        }
        seen
    }

    /// Load `out.len()` words from `start`, each atomically but not all at
    /// once. Implementations may copy in bulk.
    fn load_range(&self, start: usize, out: &mut [u32]) {
        for (k, o) in out.iter_mut().enumerate() {
            *o = self.load(start + k);
        }
    }
}

impl SharedWords for [AtomicU32] {
    fn len(&self) -> usize {
        <[AtomicU32]>::len(self)
    }
    fn load(&self, i: usize) -> u32 {
        self[i].load(Ordering::Acquire)
    }
    fn store(&self, i: usize, value: u32) {
        self[i].store(value, Ordering::Release);
    }
    fn swap(&self, i: usize, value: u32) -> u32 {
        self[i].swap(value, Ordering::AcqRel)
    }
    fn fetch_or(&self, i: usize, value: u32) -> u32 {
        self[i].fetch_or(value, Ordering::AcqRel)
    }
    fn fetch_add(&self, i: usize, value: u32) -> u32 {
        self[i].fetch_add(value, Ordering::AcqRel)
    }
    fn compare_exchange(&self, i: usize, current: u32, new: u32) -> Result<u32, u32> {
        self[i].compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
    }
    fn fetch_max(&self, i: usize, value: u32) -> u32 {
        self[i].fetch_max(value, Ordering::AcqRel)
    }
}

macro_rules! forward_shared_words {
    ($($ty:ty),*) => {$(
        impl<T: SharedWords + ?Sized> SharedWords for $ty {
            fn len(&self) -> usize { (**self).len() }
            fn load(&self, i: usize) -> u32 { (**self).load(i) }
            fn store(&self, i: usize, value: u32) { (**self).store(i, value) }
            fn swap(&self, i: usize, value: u32) -> u32 { (**self).swap(i, value) }
            fn fetch_or(&self, i: usize, value: u32) -> u32 { (**self).fetch_or(i, value) }
            fn fetch_add(&self, i: usize, value: u32) -> u32 { (**self).fetch_add(i, value) }
            fn compare_exchange(&self, i: usize, current: u32, new: u32) -> Result<u32, u32> {
                (**self).compare_exchange(i, current, new)
            }
            fn fetch_max(&self, i: usize, value: u32) -> u32 { (**self).fetch_max(i, value) }
            fn load_range(&self, start: usize, out: &mut [u32]) { (**self).load_range(start, out) }
        }
    )*};
}
forward_shared_words!(&T, Arc<T>, Box<T>);

/// Words a [`TelemetryReader`](crate::TelemetryReader) can hold: `Send` on
/// native targets, so a link stays movable to the thread that opens the
/// editor; not on wasm, whose JS-backed words can't be.
#[cfg(not(target_arch = "wasm32"))]
pub trait ReaderWords: SharedWords + Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: SharedWords + Send + ?Sized> ReaderWords for T {}

/// Words a [`TelemetryReader`](crate::TelemetryReader) can hold: `Send` on
/// native targets, so a link stays movable to the thread that opens the
/// editor; not on wasm, whose JS-backed words can't be.
#[cfg(target_arch = "wasm32")]
pub trait ReaderWords: SharedWords {}
#[cfg(target_arch = "wasm32")]
impl<T: SharedWords + ?Sized> ReaderWords for T {}
