//! The web transport ([design/engine-bridge.md], [design/hosts.md]): every channel in one
//! block of 32-bit words, shared by the UI and engine wasm modules through
//! a `SharedArrayBuffer`.
//!
//! Neither module can address the buffer as its own memory (that would need
//! shared wasm memory), so both sides reach it through [`SharedWords`]:
//! JS `Atomics` over an `Int32Array` on the web, `[AtomicU32]` in native
//! tests. [`SabUi`] and [`SabEngine`] are the same code on every target.
//!
//! # Layout, version 3
//!
//! This is a **versioned contract** between separately compiled modules,
//! like the [record codec](crate::codec). Change it only by bumping
//! [`VERSION`]. Offsets are in 32-bit words; all values little-endian, as
//! wasm is.
//!
//! ```text
//! word         field
//!    0         magic          0x5353_4142 ("SSAB")
//!    1         version        3
//!    2         params         N, the param count
//!    3         capacity       C, ring records; a power of two
//!    4         write          ring records written, mod 2^32   (UI)
//!    5         read           ring records read, mod 2^32      (engine)
//!    6         resync         sticky flag: UI sets, engine clears
//!    7         load           sticky flag: UI sets, engine clears
//!    8         overflows      events dropped on a full ring    (UI)
//!    9         telemetry      T, the telemetry block's size (a check)
//!   10         midi write     MIDI words written, mod 2^32     (page)
//!   11         midi read      MIDI words read, mod 2^32        (engine)
//!   12         midi capacity  M, MIDI words; a power of two
//!   13         midi overflows MIDI words dropped on a full ring (page)
//!   14..16     reserved       0
//!   16         ring           C records × 4 words, each a codec record
//!   16 + 4C    ui slots       N × f32 bits                     (UI)
//!   .. + N     host slots     N × f32 bits                     (engine)
//!   .. + N     dirty          ⌈N / 32⌉ bitset words: engine sets, UI clears
//!   .. + ⌈N/32⌉ telemetry     T words: the engine's output, laid out as
//!                             [`crate::telemetry`] describes   (engine)
//!   .. + T     midi           M words, each a MIDI 1.0 channel message in
//!                             its low three bytes, status first (page)
//! ```
//!
//! Version 2 added the telemetry block: the scope ring, the
//! output's peaks and the engine's meters (design/engine-bridge.md). Version 3
//! added the MIDI ring: note input from the page standing in for a
//! host (design/hosts.md), written by one [`SabMidiOut`] and decoded with
//! [`NoteEvent::from_midi`].
//!
//! **Single-writer discipline** matches the native transport's: the UI
//! writes `write`, the records, the UI slots and `overflows`, and sets
//! `resync` and `load`; the engine writes `read` and the host slots, sets
//! dirty bits and clears the flags. A record's words are written before
//! `write` is advanced past it; a host slot before its dirty bit is set.
//! JS `Atomics` are sequentially consistent, which covers both.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use crate::codec::{self, RECORD_SIZE, Record};
use crate::telemetry::{TELEMETRY_WORDS, TelemetryReader, TelemetryTap};
pub use crate::words::SharedWords;
use crate::{EngineEvent, EngineLink, NoteEvent, ParamIndex};

/// Layout version.
pub const VERSION: u32 = 3;

/// MIDI words the ring holds: far more than hands on a keyboard send in
/// one quantum.
pub const MIDI_CAPACITY: usize = 256;

/// The first word of every buffer.
pub const MAGIC: u32 = 0x5353_4142;

const MAGIC_AT: usize = 0;
const VERSION_AT: usize = 1;
const PARAMS_AT: usize = 2;
const CAPACITY_AT: usize = 3;
const WRITE_AT: usize = 4;
const READ_AT: usize = 5;
const RESYNC_AT: usize = 6;
const LOAD_AT: usize = 7;
const OVERFLOWS_AT: usize = 8;
const TELEMETRY_SIZE_AT: usize = 9;
const MIDI_WRITE_AT: usize = 10;
const MIDI_READ_AT: usize = 11;
const MIDI_CAPACITY_AT: usize = 12;
const MIDI_OVERFLOWS_AT: usize = 13;
const HEADER_WORDS: usize = 16;
const RING_AT: usize = HEADER_WORDS;
const RECORD_WORDS: usize = RECORD_SIZE / 4;

/// Where each channel sits, for a param count and ring capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SabLayout {
    params: usize,
    capacity: usize,
    midi_capacity: usize,
}

/// Why a buffer can't be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// `capacity` isn't a power of two of at least 2.
    Capacity(usize),
    /// The buffer is smaller than its layout needs.
    TooSmall { words: usize, needed: usize },
    /// The first word isn't [`MAGIC`]: not a silkscreen buffer.
    Magic(u32),
    /// Written by another layout version.
    Version(u32),
    /// Written by a build with another telemetry block size.
    Telemetry(u32),
}

impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            LayoutError::Capacity(c) => {
                write!(f, "ring capacity {c} is not a power of two of at least 2")
            }
            LayoutError::TooSmall { words, needed } => {
                write!(
                    f,
                    "shared buffer has {words} words; its layout needs {needed}"
                )
            }
            LayoutError::Magic(m) => write!(f, "not a silkscreen shared buffer (magic {m:#x})"),
            LayoutError::Telemetry(t) => write!(
                f,
                "shared buffer telemetry block is {t} words; this build's is {TELEMETRY_WORDS}; rebuild both modules together"
            ),
            LayoutError::Version(v) => write!(
                f,
                "shared buffer layout version {v}; this build reads version {VERSION}; rebuild both modules together"
            ),
        }
    }
}

impl SabLayout {
    pub fn new(params: usize, capacity: usize) -> Result<Self, LayoutError> {
        if capacity < 2 || !capacity.is_power_of_two() {
            return Err(LayoutError::Capacity(capacity));
        }
        Ok(Self {
            params,
            capacity,
            midi_capacity: MIDI_CAPACITY,
        })
    }

    /// The layout recorded in `words`' header, checked against its size.
    pub fn read(words: &impl SharedWords) -> Result<Self, LayoutError> {
        if words.len() < HEADER_WORDS {
            return Err(LayoutError::TooSmall {
                words: words.len(),
                needed: HEADER_WORDS,
            });
        }
        match words.load(MAGIC_AT) {
            MAGIC => {}
            m => return Err(LayoutError::Magic(m)),
        }
        match words.load(VERSION_AT) {
            VERSION => {}
            v => return Err(LayoutError::Version(v)),
        }
        match words.load(TELEMETRY_SIZE_AT) as usize {
            TELEMETRY_WORDS => {}
            t => return Err(LayoutError::Telemetry(t as u32)),
        }
        let mut layout = Self::new(
            words.load(PARAMS_AT) as usize,
            words.load(CAPACITY_AT) as usize,
        )?;
        layout.midi_capacity = words.load(MIDI_CAPACITY_AT) as usize;
        if layout.midi_capacity < 2 || !layout.midi_capacity.is_power_of_two() {
            return Err(LayoutError::Capacity(layout.midi_capacity));
        }
        layout.check_size(words)?;
        Ok(layout)
    }

    /// Total size in words.
    pub fn words(&self) -> usize {
        self.midi() + self.midi_capacity
    }

    /// Total size in bytes, for allocating the `SharedArrayBuffer`.
    pub fn bytes(&self) -> usize {
        self.words() * 4
    }

    pub fn params(&self) -> usize {
        self.params
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn ui_slots(&self) -> usize {
        RING_AT + self.capacity * RECORD_WORDS
    }
    fn host_slots(&self) -> usize {
        self.ui_slots() + self.params
    }
    fn dirty(&self) -> usize {
        self.host_slots() + self.params
    }
    fn telemetry(&self) -> usize {
        self.dirty() + self.params.div_ceil(32)
    }
    fn midi(&self) -> usize {
        self.telemetry() + TELEMETRY_WORDS
    }

    fn check_size(&self, words: &impl SharedWords) -> Result<(), LayoutError> {
        if words.len() < self.words() {
            return Err(LayoutError::TooSmall {
                words: words.len(),
                needed: self.words(),
            });
        }
        Ok(())
    }

    /// Write the header and seed both slot arrays with `initial`, whose
    /// length must be the param count. The ring starts empty.
    fn init(&self, words: &impl SharedWords, initial: &[f32]) -> Result<(), LayoutError> {
        assert_eq!(initial.len(), self.params, "one initial value per param");
        self.check_size(words)?;
        for i in 0..self.words() {
            words.store(i, 0);
        }
        for (i, v) in initial.iter().enumerate() {
            words.store(self.ui_slots() + i, v.to_bits());
            words.store(self.host_slots() + i, v.to_bits());
        }
        words.store(PARAMS_AT, self.params as u32);
        words.store(CAPACITY_AT, self.capacity as u32);
        words.store(TELEMETRY_SIZE_AT, TELEMETRY_WORDS as u32);
        words.store(MIDI_CAPACITY_AT, self.midi_capacity as u32);
        words.store(VERSION_AT, VERSION);
        // Last: a reader that sees the magic sees the rest.
        words.store(MAGIC_AT, MAGIC);
        Ok(())
    }
}

/// The UI's end, over shared words: an [`EngineLink`]. Main thread only.
#[derive(Debug)]
pub struct SabUi<W> {
    words: W,
    layout: SabLayout,
    telemetry: TelemetryReader,
    /// The MIDI ring's single writer was handed out.
    midi_taken: bool,
}

impl<W: crate::words::ReaderWords + Clone + 'static> SabUi<W> {
    /// Lay out `words` (at least `layout.words()` long) and seed every
    /// value with `initial`. The UI creates the buffer; the engine attaches
    /// with [`SabEngine::attach`].
    pub fn create(words: W, layout: SabLayout, initial: &[f32]) -> Result<Self, LayoutError> {
        layout.init(&words, initial)?;
        let telemetry = TelemetryReader::over(Box::new(words.clone()), layout.telemetry());
        Ok(Self {
            words,
            layout,
            telemetry,
            midi_taken: false,
        })
    }

    /// The MIDI ring's writer, for the page standing in for a host
    /// (design/hosts.md). The ring has one writer, so this hands it out once and
    /// returns `None` after.
    pub fn midi_out(&mut self) -> Option<SabMidiOut<W>> {
        if std::mem::replace(&mut self.midi_taken, true) {
            return None;
        }
        Some(SabMidiOut {
            words: self.words.clone(),
            layout: self.layout,
        })
    }
}

/// Writes MIDI 1.0 channel messages into the MIDI ring, for the engine's
/// [`Engine::note`](crate::Engine::note). The ring's only writer; from
/// [`SabUi::midi_out`].
#[derive(Debug)]
pub struct SabMidiOut<W> {
    words: W,
    layout: SabLayout,
}

impl<W: SharedWords> SabMidiOut<W> {
    /// Queue one message: status, then two data bytes (unused ones 0).
    /// Returns `false`, dropping it and counting the drop, if the ring is
    /// full.
    pub fn send(&mut self, message: [u8; 3]) -> bool {
        let l = &self.layout;
        let write = self.words.load(MIDI_WRITE_AT);
        let read = self.words.load(MIDI_READ_AT);
        if write.wrapping_sub(read) as usize >= l.midi_capacity {
            self.words.fetch_add(MIDI_OVERFLOWS_AT, 1);
            return false;
        }
        let at = l.midi() + (write as usize & (l.midi_capacity - 1));
        let [status, d1, d2] = message;
        self.words
            .store(at, u32::from_le_bytes([status, d1, d2, 0]));
        self.words.store(MIDI_WRITE_AT, write.wrapping_add(1));
        true
    }

    /// Messages dropped because the ring was full.
    pub fn overflow_count(&self) -> u32 {
        self.words.load(MIDI_OVERFLOWS_AT)
    }
}

impl<W: SharedWords> SabUi<W> {
    pub fn layout(&self) -> SabLayout {
        self.layout
    }

    /// The shared words, e.g. to hand their buffer to the engine.
    pub fn words(&self) -> &W {
        &self.words
    }

    /// Events dropped because the ring was full (each forced a resync).
    pub fn overflow_count(&self) -> u32 {
        self.words.load(OVERFLOWS_AT)
    }
}

impl<W: SharedWords> EngineLink for SabUi<W> {
    fn send(&mut self, event: EngineEvent) {
        let l = &self.layout;
        if let EngineEvent::ParamValue { index, value } = event {
            // The slot first, so a resync always sees this value.
            self.words
                .store(l.ui_slots() + index.index(), value.to_bits());
        }
        let write = self.words.load(WRITE_AT);
        let read = self.words.load(READ_AT);
        if write.wrapping_sub(read) as usize >= l.capacity {
            self.words.fetch_add(OVERFLOWS_AT, 1);
            self.words.store(RESYNC_AT, 1);
            return;
        }
        let at = RING_AT + (write as usize & (l.capacity - 1)) * RECORD_WORDS;
        let record = codec::encode(&event);
        for (k, chunk) in record.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            self.words.store(at + k, word);
        }
        self.words.store(WRITE_AT, write.wrapping_add(1));
    }

    fn poll_host(&mut self, report: &mut dyn FnMut(ParamIndex, f32)) {
        let l = &self.layout;
        for w in 0..l.params.div_ceil(32) {
            let mut bits = self.words.swap(l.dirty() + w, 0);
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let i = w * 32 + bit;
                let value = f32::from_bits(self.words.load(l.host_slots() + i));
                report(ParamIndex(i as u32), value);
            }
        }
    }

    fn load(&mut self, values: &[f32]) {
        let l = &self.layout;
        for (i, v) in values.iter().take(l.params).enumerate() {
            self.words.store(l.ui_slots() + i, v.to_bits());
        }
        self.words.store(LOAD_AT, 1);
    }

    fn telemetry(&mut self) -> Option<&mut TelemetryReader> {
        Some(&mut self.telemetry)
    }
}

/// The engine's end, over shared words. The same API as the native
/// [`EngineEnd`](crate::EngineEnd). Audio thread only; no method but
/// [`attach`](Self::attach) allocates, and none blocks.
#[derive(Debug)]
pub struct SabEngine<W> {
    words: W,
    layout: SabLayout,
    /// The engine's view of every value, from either source.
    current: Box<[f32]>,
    telemetry: TelemetryTap<W>,
}

impl<W: SharedWords + Clone> SabEngine<W> {
    /// Attach to a buffer the UI laid out with [`SabUi::create`]. Checks
    /// the magic, version and sizes, so modules from different builds fail
    /// here rather than misread each other.
    pub fn attach(words: W) -> Result<Self, LayoutError> {
        let layout = SabLayout::read(&words)?;
        let current = (0..layout.params)
            .map(|i| f32::from_bits(words.load(layout.ui_slots() + i)))
            .collect();
        let telemetry = TelemetryTap::over(words.clone(), layout.telemetry());
        Ok(Self {
            words,
            layout,
            current,
            telemetry,
        })
    }
}

impl<W: SharedWords> SabEngine<W> {
    /// Where the host records each block's output for the editor
    /// ([design/engine-bridge.md]), as [`EngineEnd::telemetry`](crate::EngineEnd::telemetry).
    ///
    /// [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md
    pub fn telemetry(&mut self) -> &mut TelemetryTap<W> {
        &mut self.telemetry
    }

    /// Discard everything queued.
    fn discard(&mut self) {
        let write = self.words.load(WRITE_AT);
        self.words.store(READ_AT, write);
    }

    fn ui_slot(&self, i: usize) -> f32 {
        f32::from_bits(self.words.load(self.layout.ui_slots() + i))
    }

    /// Deliver pending UI events in order. After an overflow, the queue is
    /// discarded and every param is delivered as a `ParamValue` from the UI
    /// slots instead. Returns `true` if that happened, so the caller can
    /// close any gesture it still has open. Records with an unknown tag are
    /// skipped.
    pub fn drain(&mut self, mut deliver: impl FnMut(EngineEvent)) -> bool {
        let resynced = self.words.swap(RESYNC_AT, 0) != 0;
        if resynced {
            self.discard();
            for i in 0..self.layout.params {
                let value = self.ui_slot(i);
                self.current[i] = value;
                deliver(EngineEvent::ParamValue {
                    index: ParamIndex(i as u32),
                    value,
                });
            }
        }
        let l = self.layout;
        let write = self.words.load(WRITE_AT);
        let mut read = self.words.load(READ_AT);
        while read != write {
            let at = RING_AT + (read as usize & (l.capacity - 1)) * RECORD_WORDS;
            let mut record: Record = [0; RECORD_SIZE];
            for k in 0..RECORD_WORDS {
                record[k * 4..k * 4 + 4].copy_from_slice(&self.words.load(at + k).to_le_bytes());
            }
            read = read.wrapping_add(1);
            if let Ok(event) = codec::decode(&record) {
                if let EngineEvent::ParamValue { index, value } = event
                    && let Some(v) = self.current.get_mut(index.index())
                {
                    *v = value;
                }
                deliver(event);
            }
        }
        self.words.store(READ_AT, read);
        resynced
    }

    /// Deliver the page's pending MIDI messages in order, decoded with
    /// [`NoteEvent::from_midi`]; words that don't decode are skipped.
    pub fn drain_midi(&mut self, mut deliver: impl FnMut(NoteEvent)) {
        let l = self.layout;
        let write = self.words.load(MIDI_WRITE_AT);
        let mut read = self.words.load(MIDI_READ_AT);
        while read != write {
            let word = self
                .words
                .load(l.midi() + (read as usize & (l.midi_capacity - 1)));
            let [status, d1, d2, _] = word.to_le_bytes();
            read = read.wrapping_add(1);
            if let Some(event) = NoteEvent::from_midi([status, d1, d2]) {
                deliver(event);
            }
        }
        self.words.store(MIDI_READ_AT, read);
    }

    /// Apply a pending state load, if there is one: discard the queue, and
    /// hand every param's value to `apply`. Not UI edits: don't report them
    /// to the host. Call before [`drain`](Self::drain).
    pub fn take_load(&mut self, mut apply: impl FnMut(ParamIndex, f32)) -> bool {
        if self.words.swap(LOAD_AT, 0) == 0 {
            return false;
        }
        self.discard();
        for i in 0..self.layout.params {
            let value = self.ui_slot(i);
            self.current[i] = value;
            apply(ParamIndex(i as u32), value);
        }
        true
    }

    /// The value to seed a fresh engine with: [`read`](Self::read), or the
    /// pending load's value if one is waiting.
    pub fn seed(&self, index: ParamIndex) -> f32 {
        if self.words.load(LOAD_AT) != 0 {
            self.ui_slot(index.index())
        } else {
            self.read(index)
        }
    }

    /// Record a value that came from the host, for the editor to pick up.
    /// Never call this for values from [`drain`](Self::drain).
    pub fn publish_host(&mut self, index: ParamIndex, value: f32) {
        let i = index.index();
        let l = self.layout;
        self.current[i] = value;
        self.words.store(l.host_slots() + i, value.to_bits());
        self.words.fetch_or(l.dirty() + i / 32, 1 << (i % 32));
    }

    /// The latest value of `index` from either source.
    pub fn read(&self, index: ParamIndex) -> f32 {
        self.current[index.index()]
    }

    pub fn param_count(&self) -> usize {
        self.layout.params
    }

    /// Events dropped because the ring was full.
    pub fn overflow_count(&self) -> u32 {
        self.words.load(OVERFLOWS_AT)
    }
}

/// The engine module's view of the buffer: functions the worklet supplies
/// as the wasm import module `silkscreen_sab`, each `Atomics` on an
/// `Int32Array` over the `SharedArrayBuffer`:
///
/// ```js
/// const w = new Int32Array(sab);
/// const silkscreen_sab = {
///   len: () => w.length,
///   load: (i) => Atomics.load(w, i),
///   store: (i, v) => { Atomics.store(w, i, v); },
///   swap: (i, v) => Atomics.exchange(w, i, v),
///   fetch_or: (i, v) => Atomics.or(w, i, v),
///   fetch_add: (i, v) => Atomics.add(w, i, v),
///   compare_exchange: (i, c, v) => Atomics.compareExchange(w, i, c, v),
/// };
/// ```
///
/// One buffer per engine module instance. A call is a wasm → JS crossing
/// with integer arguments, which allocates nothing: four per idle block,
/// plus four per record, and one per param on a resync or load.
///
/// Both ends run the Rust above, so the ring exists once; the worklet
/// only supplies these six functions. (A worklet that ran the ring in JS
/// and bulk-copied records into wasm would need the ring twice, kept in
/// step by a wire-format document and mirrored golden tables.)
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportedWords;

#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod imported {
    #[link(wasm_import_module = "silkscreen_sab")]
    unsafe extern "C" {
        pub safe fn len() -> u32;
        pub safe fn load(i: u32) -> u32;
        pub safe fn store(i: u32, value: u32);
        pub safe fn swap(i: u32, value: u32) -> u32;
        pub safe fn fetch_or(i: u32, value: u32) -> u32;
        pub safe fn fetch_add(i: u32, value: u32) -> u32;
        /// The value before, as `Atomics.compareExchange` returns it.
        pub safe fn compare_exchange(i: u32, current: u32, new: u32) -> u32;
    }
}

#[cfg(target_arch = "wasm32")]
impl SharedWords for ImportedWords {
    fn len(&self) -> usize {
        imported::len() as usize
    }
    fn load(&self, i: usize) -> u32 {
        imported::load(i as u32)
    }
    fn store(&self, i: usize, value: u32) {
        imported::store(i as u32, value);
    }
    fn swap(&self, i: usize, value: u32) -> u32 {
        imported::swap(i as u32, value)
    }
    fn fetch_or(&self, i: usize, value: u32) -> u32 {
        imported::fetch_or(i as u32, value)
    }
    fn fetch_add(&self, i: usize, value: u32) -> u32 {
        imported::fetch_add(i as u32, value)
    }
    fn compare_exchange(&self, i: usize, current: u32, new: u32) -> Result<u32, u32> {
        let before = imported::compare_exchange(i as u32, current, new);
        if before == current {
            Ok(before)
        } else {
            Err(before)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const A: ParamIndex = ParamIndex(0);
    const B: ParamIndex = ParamIndex(1);

    fn buffer(layout: SabLayout) -> Arc<[AtomicU32]> {
        (0..layout.words()).map(|_| AtomicU32::new(0)).collect()
    }

    type Words = Arc<[AtomicU32]>;

    fn pair(initial: &[f32], capacity: usize) -> (SabUi<Words>, SabEngine<Words>) {
        let layout = SabLayout::new(initial.len(), capacity).unwrap();
        let words = buffer(layout);
        let ui = SabUi::create(words.clone(), layout, initial).unwrap();
        let engine = SabEngine::attach(words).unwrap();
        (ui, engine)
    }

    fn drained(engine: &mut SabEngine<Words>) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        engine.drain(|e| out.push(e));
        out
    }

    fn polled(ui: &mut SabUi<Words>) -> Vec<(ParamIndex, f32)> {
        let mut out = Vec::new();
        ui.poll_host(&mut |p, v| out.push((p, v)));
        out
    }

    fn value(index: ParamIndex, value: f32) -> EngineEvent {
        EngineEvent::ParamValue { index, value }
    }

    #[test]
    fn layout_offsets_follow_the_documented_table() {
        let l = SabLayout::new(40, 8).unwrap();
        assert_eq!(RING_AT, 16);
        assert_eq!(l.ui_slots(), 16 + 32);
        assert_eq!(l.host_slots(), 48 + 40);
        assert_eq!(l.dirty(), 88 + 40);
        assert_eq!(l.telemetry(), 128 + 2);
        assert_eq!(l.midi(), 130 + TELEMETRY_WORDS);
        assert_eq!(l.words(), 130 + TELEMETRY_WORDS + MIDI_CAPACITY);
        assert_eq!(l.bytes(), l.words() * 4);
    }

    #[test]
    fn the_header_is_written_magic_last_and_read_back() {
        let (ui, _) = pair(&[0.5; 3], 4);
        let w = &ui.words;
        assert_eq!(
            (0..14).map(|i| w.load(i)).collect::<Vec<_>>(),
            [
                MAGIC,
                3,
                3,
                4,
                0,
                0,
                0,
                0,
                0,
                TELEMETRY_WORDS as u32,
                0,
                0,
                MIDI_CAPACITY as u32,
                0
            ]
        );
        assert_eq!(SabLayout::read(w), Ok(ui.layout()));
    }

    #[test]
    fn mismatched_buffers_are_refused_with_the_reason() {
        assert_eq!(SabLayout::new(4, 6), Err(LayoutError::Capacity(6)));
        assert_eq!(SabLayout::new(4, 1), Err(LayoutError::Capacity(1)));
        let layout = SabLayout::new(4, 4).unwrap();
        let words = buffer(layout);
        assert_eq!(
            SabEngine::attach(words.clone()).unwrap_err(),
            LayoutError::Magic(0)
        );
        SabUi::create(words.clone(), layout, &[0.0; 4]).unwrap();
        words.store(VERSION_AT, VERSION + 1);
        let err = SabEngine::attach(words.clone()).unwrap_err();
        assert_eq!(err, LayoutError::Version(VERSION + 1));
        assert!(err.to_string().contains("rebuild both modules"));
        let short: Arc<[AtomicU32]> = (0..layout.words() - 1).map(|_| AtomicU32::new(0)).collect();
        assert!(matches!(
            SabUi::create(short, layout, &[0.0; 4]),
            Err(LayoutError::TooSmall { .. })
        ));
    }

    #[test]
    fn a_telemetry_size_mismatch_is_refused() {
        let layout = SabLayout::new(1, 4).unwrap();
        let words = buffer(layout);
        SabUi::create(words.clone(), layout, &[0.0]).unwrap();
        words.store(TELEMETRY_SIZE_AT, 99);
        let err = SabEngine::attach(words).unwrap_err();
        assert_eq!(err, LayoutError::Telemetry(99));
        assert!(err.to_string().contains("rebuild both modules"));
    }

    #[test]
    fn telemetry_crosses_the_buffer() {
        use crate::telemetry::SCOPE_CAPACITY;
        let (mut ui, mut engine) = pair(&[0.0; 3], 4);
        engine.telemetry().write(&[[0.5, -1.0], [0.25, 0.5]]);
        engine.telemetry().write_engine_peak(2, [0.1, -0.3]);
        let reader = ui.telemetry().unwrap();
        let mut scope = [9.0; SCOPE_CAPACITY];
        assert!(reader.scope(&mut scope));
        assert_eq!(&scope[SCOPE_CAPACITY - 2..], &[0.375, -0.25]);
        assert_eq!(reader.take_peaks().levels, [1.0, 0.5]);
        assert_eq!(reader.take_engine_peaks(2), [0.1, 0.3]);
        assert_eq!(reader.written(), 2);
        // The rest of the buffer is untouched.
        assert!(drained(&mut engine).is_empty());
        assert!(polled(&mut ui).is_empty());
    }

    fn midi_drained(engine: &mut SabEngine<Words>) -> Vec<NoteEvent> {
        let mut out = Vec::new();
        engine.drain_midi(|e| out.push(e));
        out
    }

    #[test]
    fn midi_arrives_in_order_decoded() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        let mut midi = ui.midi_out().unwrap();
        assert!(ui.midi_out().is_none(), "one writer");
        assert!(midi.send([0x90, 60, 100]));
        assert!(midi.send([0xE0, 0, 64]));
        assert!(midi.send([0x80, 60, 0]));
        assert_eq!(
            midi_drained(&mut engine),
            [
                NoteEvent::from_midi([0x90, 60, 100]).unwrap(),
                NoteEvent::from_midi([0xE0, 0, 64]).unwrap(),
                NoteEvent::from_midi([0x80, 60, 0]).unwrap(),
            ]
        );
        assert!(midi_drained(&mut engine).is_empty());
        // The UI event ring is apart.
        assert!(drained(&mut engine).is_empty());
    }

    #[test]
    fn the_midi_ring_wraps_drops_when_full_and_skips_garbage() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        let mut midi = ui.midi_out().unwrap();
        ui.words.store(MIDI_WRITE_AT, u32::MAX - 1);
        ui.words.store(MIDI_READ_AT, u32::MAX - 1);
        for k in 0..MIDI_CAPACITY {
            assert!(midi.send([0x90, (k % 128) as u8, 1]));
        }
        assert!(!midi.send([0x90, 1, 1]), "full");
        assert_eq!(midi.overflow_count(), 1);
        assert_eq!(midi_drained(&mut engine).len(), MIDI_CAPACITY);
        // A data byte where a status byte belongs doesn't decode.
        assert!(midi.send([0x40, 1, 1]));
        assert!(midi.send([0x91, 62, 127]));
        assert_eq!(
            midi_drained(&mut engine),
            [NoteEvent::On {
                channel: 1,
                key: 62,
                velocity: 1.0
            }]
        );
    }

    #[test]
    fn events_arrive_in_order() {
        let (mut ui, mut engine) = pair(&[0.0, 0.0], 16);
        let events = [
            EngineEvent::GestureBegin { index: A },
            value(A, 0.25),
            value(A, 0.5),
            EngineEvent::GestureEnd { index: A },
        ];
        for e in events {
            ui.send(e);
        }
        assert_eq!(drained(&mut engine), events);
        assert_eq!(engine.read(A), 0.5);
        assert!(drained(&mut engine).is_empty());
    }

    #[test]
    fn initial_values_seed_the_engine() {
        let (_ui, engine) = pair(&[1.5, -3.0], 4);
        assert_eq!((engine.read(A), engine.read(B)), (1.5, -3.0));
        assert_eq!(engine.param_count(), 2);
    }

    #[test]
    fn the_ring_wraps_and_so_do_its_counters() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        // Start the monotonic counters just short of 2^32.
        ui.words.store(WRITE_AT, u32::MAX - 5);
        ui.words.store(READ_AT, u32::MAX - 5);
        for round in 0..5 {
            let batch: Vec<_> = (0..3).map(|k| value(A, (round * 3 + k) as f32)).collect();
            for &e in &batch {
                ui.send(e);
            }
            assert_eq!(drained(&mut engine), batch, "round {round}");
        }
        assert_eq!(ui.overflow_count(), 0);
        assert!(ui.words.load(WRITE_AT) < 16, "wrapped past zero");
    }

    #[test]
    fn overflow_resyncs_to_latest_ui_values() {
        let (mut ui, mut engine) = pair(&[0.0, 0.0], 4);
        for i in 0..10 {
            ui.send(value(A, i as f32));
        }
        ui.send(value(B, 7.0));
        assert_eq!(ui.overflow_count(), 7);
        assert_eq!(engine.overflow_count(), 7);

        let mut events = Vec::new();
        assert!(engine.drain(|e| events.push(e)), "reports the resync");
        assert_eq!(events, [value(A, 9.0), value(B, 7.0)]);
        assert_eq!((engine.read(A), engine.read(B)), (9.0, 7.0));
        assert!(!engine.drain(|_| {}));
        ui.send(EngineEvent::GestureBegin { index: A });
        assert_eq!(
            drained(&mut engine),
            [EngineEvent::GestureBegin { index: A }]
        );
    }

    #[test]
    fn host_values_are_reported_once_per_publish() {
        let (mut ui, mut engine) = pair(&[0.0; 70], 4);
        engine.publish_host(ParamIndex(3), 0.5);
        engine.publish_host(ParamIndex(69), 0.25);
        engine.publish_host(ParamIndex(3), 0.75);
        assert_eq!(
            polled(&mut ui),
            [(ParamIndex(3), 0.75), (ParamIndex(69), 0.25)]
        );
        assert!(polled(&mut ui).is_empty());
        assert_eq!(engine.read(ParamIndex(3)), 0.75);
    }

    #[test]
    fn a_repeated_host_value_is_still_reported() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        engine.publish_host(A, 0.5);
        assert_eq!(polled(&mut ui), [(A, 0.5)]);
        ui.send(value(A, 0.9));
        drained(&mut engine);
        engine.publish_host(A, 0.5);
        assert_eq!(polled(&mut ui), [(A, 0.5)]);
    }

    #[test]
    fn a_load_supersedes_queued_events_and_is_not_a_ui_edit() {
        let (mut ui, mut engine) = pair(&[0.0, 0.0], 16);
        ui.send(EngineEvent::GestureBegin { index: A });
        ui.send(value(A, 0.9));
        ui.load(&[0.25, 0.5]);
        assert_eq!((engine.seed(A), engine.seed(B)), (0.25, 0.5));
        assert_eq!(engine.read(A), 0.0, "not applied until taken");

        let mut loaded = Vec::new();
        assert!(engine.take_load(|p, v| loaded.push((p, v))));
        assert_eq!(loaded, [(A, 0.25), (B, 0.5)]);
        assert!(drained(&mut engine).is_empty(), "the queue was discarded");
        assert!(!engine.take_load(|_, _| {}), "taken once");
        assert!(polled(&mut ui).is_empty(), "not a host change either");
        ui.send(value(B, 0.75));
        assert_eq!(drained(&mut engine), [value(B, 0.75)]);
    }

    #[test]
    fn ui_values_are_never_reported_as_host_values() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        ui.send(value(A, 0.9));
        drained(&mut engine);
        assert!(polled(&mut ui).is_empty());
    }

    #[test]
    fn records_with_unknown_tags_are_skipped() {
        let (mut ui, mut engine) = pair(&[0.0], 4);
        ui.send(value(A, 0.5));
        ui.send(value(A, 0.75));
        // Corrupt the first record's tag.
        ui.words.store(RING_AT, 99);
        assert_eq!(drained(&mut engine), [value(A, 0.75)]);
    }

    #[test]
    fn final_state_survives_concurrent_overflow() {
        const PARAMS: u32 = 8;
        let (mut ui, mut engine) = pair(&[0.0; PARAMS as usize], 32);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let engine_thread = {
            let done = done.clone();
            std::thread::spawn(move || {
                loop {
                    let finished = done.load(Ordering::Acquire);
                    engine.drain(|_| {});
                    if finished {
                        break;
                    }
                    std::thread::yield_now();
                }
                engine
            })
        };
        for i in 1..=10_000u32 {
            ui.send(value(ParamIndex(i % PARAMS), i as f32));
        }
        done.store(true, Ordering::Release);
        let engine = engine_thread.join().unwrap();
        for p in 0..PARAMS {
            // The last value sent to p: the largest i ≤ 10 000 with i % 8 == p.
            let last = (1..=10_000u32).rev().find(|i| i % PARAMS == p).unwrap();
            assert_eq!(engine.read(ParamIndex(p)), last as f32, "param {p}");
        }
    }
}
