//! The audio thread's floating-point mode ([design/engine-bridge.md]).
//!
//! Subnormal arithmetic costs up to ~100× on x86 and wrecks deadlines in
//! decaying feedback paths: a filter's state, an envelope's tail. Rather
//! than every engine checking its own state, the host holds a
//! [`DenormalGuard`] for each process call and the CPU flushes them.
//! Engine code has no ad hoc flushes.
//!
//! The control word is per thread, and a host may call `process` on a
//! different thread each time, so the guard is taken per call, not once at
//! activation. It restores the previous mode on drop, so the host and the
//! plugins after this one see the mode they set.
//!
//! - **`x86_64`:** MXCSR FTZ (bit 15, subnormal results become zero) and DAZ
//!   (bit 6, subnormal inputs read as zero). Both: FTZ alone still pays
//!   for a subnormal that arrives as an input.
//! - **`aarch64`, and `arm64ec` (Windows on Arm hosts such as REAPER):**
//!   FPCR FZ (bit 24), which flushes inputs and results. ARM64EC code runs
//!   natively on the Arm FPCR.
//! - **Elsewhere, wasm included:** a no-op. WebAssembly has no FP control
//!   word; the web target measures before it flushes anything.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

/// MXCSR flush-to-zero and denormals-are-zero.
#[cfg(target_arch = "x86_64")]
const MXCSR_FTZ_DAZ: u32 = 0x8000 | 0x0040;

/// FPCR flush-to-zero.
#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec"))]
const FPCR_FZ: u64 = 1 << 24;

/// Flushes subnormals to zero on this thread until dropped. Bind it for the
/// whole process call: `let _fp = DenormalGuard::new();`.
#[derive(Debug)]
#[must_use = "the mode is restored when the guard drops; bind it for the whole block"]
pub struct DenormalGuard {
    #[cfg(target_arch = "x86_64")]
    previous: u32,
    #[cfg(any(target_arch = "aarch64", target_arch = "arm64ec"))]
    previous: u64,
}

impl DenormalGuard {
    #[inline]
    pub fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let previous = control::read();
            control::write(previous | MXCSR_FTZ_DAZ);
            Self { previous }
        }
        #[cfg(any(target_arch = "aarch64", target_arch = "arm64ec"))]
        {
            let previous = control::read();
            control::write(previous | FPCR_FZ);
            Self { previous }
        }
        #[cfg(not(any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm64ec"
        )))]
        {
            Self {}
        }
    }
}

impl Default for DenormalGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DenormalGuard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm64ec"
        ))]
        control::write(self.previous);
    }
}

/// The control word itself. The only unsafe code in the crate: reading
/// and writing it touches no memory but the local.
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
mod control {
    pub fn read() -> u32 {
        let mut mxcsr: u32 = 0;
        // SAFETY: stmxcsr stores the 32-bit MXCSR to a valid local.
        unsafe {
            core::arch::asm!("stmxcsr [{}]", in(reg) &raw mut mxcsr, options(nostack, preserves_flags));
        }
        mxcsr
    }

    pub fn write(mxcsr: u32) {
        // SAFETY: ldmxcsr loads MXCSR from a valid local; FTZ/DAZ and the
        // restored host value are valid modes.
        unsafe {
            core::arch::asm!("ldmxcsr [{}]", in(reg) &raw const mxcsr, options(nostack, preserves_flags));
        }
    }
}

#[cfg(any(target_arch = "aarch64", target_arch = "arm64ec"))]
#[allow(unsafe_code)]
mod control {
    pub fn read() -> u64 {
        let fpcr: u64;
        // SAFETY: reading FPCR has no side effects.
        unsafe {
            core::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
        }
        fpcr
    }

    pub fn write(fpcr: u64) {
        // SAFETY: FZ, and the restored host value, are valid modes.
        unsafe {
            core::arch::asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(all(
    test,
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm64ec"
    )
))]
mod tests {
    use core::hint::black_box;

    use super::*;

    /// Half the smallest normal: a subnormal result, unless flushed.
    fn halve_smallest_normal() -> (f32, f64) {
        (
            black_box(f32::MIN_POSITIVE) * black_box(0.5),
            black_box(f64::MIN_POSITIVE) * black_box(0.5),
        )
    }

    #[test]
    fn subnormal_results_flush_to_zero_under_the_guard() {
        let (f, d) = halve_smallest_normal();
        assert!(
            f.is_subnormal() && d.is_subnormal(),
            "precondition: no flush by default"
        );
        {
            let _fp = DenormalGuard::new();
            assert_eq!(halve_smallest_normal(), (0.0, 0.0));
        }
        let (f, d) = halve_smallest_normal();
        assert!(
            f.is_subnormal() && d.is_subnormal(),
            "the mode is restored on drop"
        );
    }

    /// Subnormal inputs read as zero too, which FTZ alone does not give on
    /// x86.
    #[test]
    fn subnormal_inputs_read_as_zero_under_the_guard() {
        let tiny = black_box(f64::from_bits(1));
        let _fp = DenormalGuard::new();
        assert_eq!(black_box(tiny) * black_box(4.0), 0.0);
    }

    #[test]
    fn the_previous_mode_is_restored_exactly() {
        let before = control::read();
        {
            let _outer = DenormalGuard::new();
            let _inner = DenormalGuard::new();
        }
        assert_eq!(control::read(), before);
    }
}
