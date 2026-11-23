//! The 16-byte wire record ([design/engine-bridge.md]).
//!
//! This layout is a **versioned contract** between separately compiled
//! modules (on the web, the UI and engine wasm modules share it through a
//! `SharedArrayBuffer`). Change it only by bumping [`VERSION`].
//!
//! ```text
//! offset  size  field
//!      0     1  tag        1 = ParamValue, 2 = GestureBegin, 3 = GestureEnd,
//!                          4 = Shown, 5 = PatchLoad (0 is reserved: never a
//!                          valid record)
//!      1     3  reserved   written as 0, ignored on read
//!      4     4  index      u32, little-endian (Shown: the page group)
//!      8     4  value      f32 bits, little-endian (0 for gestures;
//!                          Shown: the page, a u32)
//!     12     4  reserved   written as 0, ignored on read
//! ```
//!
//! Version 2 added `Shown`; version 3 `PatchLoad`, whose
//! index and value are 0.
//!
//! [design/engine-bridge.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/engine-bridge.md

use crate::{EngineEvent, ParamIndex};

/// Layout version of [`Record`].
pub const VERSION: u32 = 3;

/// Size of one record in bytes.
pub const RECORD_SIZE: usize = 16;

pub type Record = [u8; RECORD_SIZE];

const TAG_PARAM_VALUE: u8 = 1;
const TAG_GESTURE_BEGIN: u8 = 2;
const TAG_GESTURE_END: u8 = 3;
const TAG_SHOWN: u8 = 4;
const TAG_PATCH_LOAD: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnknownTag(u8),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::UnknownTag(t) => write!(f, "unknown engine event tag {t}"),
        }
    }
}

pub fn encode(event: &EngineEvent) -> Record {
    let (tag, index, value) = match *event {
        EngineEvent::ParamValue { index, value } => (TAG_PARAM_VALUE, index.0, value.to_bits()),
        EngineEvent::GestureBegin { index } => (TAG_GESTURE_BEGIN, index.0, 0),
        EngineEvent::GestureEnd { index } => (TAG_GESTURE_END, index.0, 0),
        EngineEvent::Shown { group, page } => (TAG_SHOWN, u32::from(group), u32::from(page)),
        EngineEvent::PatchLoad => (TAG_PATCH_LOAD, 0, 0),
    };
    let mut r = [0u8; RECORD_SIZE];
    r[0] = tag;
    r[4..8].copy_from_slice(&index.to_le_bytes());
    r[8..12].copy_from_slice(&value.to_le_bytes());
    r
}

pub fn decode(record: &Record) -> Result<EngineEvent, DecodeError> {
    let word = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
    let bits = u32::from_le_bytes([record[8], record[9], record[10], record[11]]);
    let index = ParamIndex(word);
    let value = f32::from_bits(bits);
    match record[0] {
        TAG_PARAM_VALUE => Ok(EngineEvent::ParamValue { index, value }),
        TAG_GESTURE_BEGIN => Ok(EngineEvent::GestureBegin { index }),
        TAG_GESTURE_END => Ok(EngineEvent::GestureEnd { index }),
        // Group and page were u16s when encoded.
        TAG_SHOWN => Ok(EngineEvent::Shown {
            group: word as u16,
            page: bits as u16,
        }),
        TAG_PATCH_LOAD => Ok(EngineEvent::PatchLoad),
        t => Err(DecodeError::UnknownTag(t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(e: EngineEvent) -> EngineEvent {
        decode(&encode(&e)).unwrap()
    }

    #[test]
    fn every_variant_round_trips() {
        let i = ParamIndex(0xDEAD_BEEF);
        for e in [
            EngineEvent::ParamValue {
                index: i,
                value: -12.5,
            },
            EngineEvent::GestureBegin { index: i },
            EngineEvent::GestureEnd {
                index: ParamIndex(0),
            },
            EngineEvent::Shown {
                group: 3,
                page: 0xBEEF,
            },
            EngineEvent::PatchLoad,
        ] {
            assert_eq!(round_trip(e), e);
        }
    }

    #[test]
    fn value_bits_are_preserved_exactly() {
        let bits = [
            0.0f32.to_bits(),
            (-0.0f32).to_bits(),
            f32::INFINITY.to_bits(),
            f32::MIN_POSITIVE.to_bits(),
            0x7fc0_0001, // NaN with payload
            0xffc0_0000, // negative quiet NaN
        ];
        for b in bits {
            let e = EngineEvent::ParamValue {
                index: ParamIndex(7),
                value: f32::from_bits(b),
            };
            match round_trip(e) {
                EngineEvent::ParamValue { value, .. } => assert_eq!(value.to_bits(), b),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn layout_is_little_endian_with_zeroed_reserved_bytes() {
        let r = encode(&EngineEvent::ParamValue {
            index: ParamIndex(0x0403_0201),
            value: f32::from_bits(0x0807_0605),
        });
        assert_eq!(r, [1, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0]);
        let r = encode(&EngineEvent::GestureEnd {
            index: ParamIndex(1),
        });
        assert_eq!(r, [3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn unknown_tags_are_errors_not_panics() {
        assert_eq!(decode(&[0; 16]), Err(DecodeError::UnknownTag(0)));
        let mut r = [0; 16];
        r[0] = 200;
        assert_eq!(decode(&r), Err(DecodeError::UnknownTag(200)));
    }

    #[test]
    fn reserved_bytes_are_ignored_on_read() {
        let mut r = encode(&EngineEvent::GestureBegin {
            index: ParamIndex(9),
        });
        r[1] = 0xff;
        r[15] = 0xff;
        assert_eq!(
            decode(&r),
            Ok(EngineEvent::GestureBegin {
                index: ParamIndex(9)
            })
        );
    }
}
