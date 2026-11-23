//! Which wasm memory, and where in it, a JS view of a Rust slice was made.

use wasm_bindgen::{JsCast, JsValue};

/// Identifies a JS view of a slice of wasm memory: the memory's
/// `ArrayBuffer` at the time, and the slice's address and length.
///
/// When wasm memory grows, its old `ArrayBuffer` is detached and every
/// view over it (an `ImageData`'s pixels, say) reads as empty. A new
/// buffer replaces it, so comparing buffers tells whether a view made
/// earlier still sees the slice.
#[derive(Debug, Clone)]
pub struct MemoryBinding {
    buffer: JsValue,
    addr: usize,
    len: usize,
}

impl MemoryBinding {
    /// The binding a view of `data`, made now, has.
    pub fn of(data: &[u8]) -> Self {
        Self {
            buffer: buffer(),
            addr: data.as_ptr() as usize,
            len: data.len(),
        }
    }

    /// Whether a view made under this binding still sees `data`.
    pub fn sees(&self, data: &[u8]) -> bool {
        self.addr == data.as_ptr() as usize && self.len == data.len() && self.buffer == buffer()
    }
}

/// The current `ArrayBuffer` of this module's memory.
fn buffer() -> JsValue {
    wasm_bindgen::memory()
        .unchecked_into::<js_sys::WebAssembly::Memory>()
        .buffer()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn a_binding_survives_until_memory_grows() {
        let frame = vec![0u8; 64 * 64 * 4];
        let binding = MemoryBinding::of(&frame);
        assert!(binding.sees(&frame));
        // Force a grow: more than wasm's initial memory in one block.
        let pages = js_sys::WebAssembly::Memory::from(wasm_bindgen::memory()).grow(0);
        let big = vec![1u8; (pages as usize + 16) * 65_536];
        assert!(
            !binding.sees(&frame),
            "memory grew; the old view is detached"
        );
        assert!(MemoryBinding::of(&frame).sees(&frame));
        drop(big);
    }

    #[wasm_bindgen_test]
    fn a_reallocated_slice_needs_a_new_binding() {
        let a = vec![0u8; 16];
        let b = vec![0u8; 16];
        assert!(!MemoryBinding::of(&a).sees(&b));
    }
}
