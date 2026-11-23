//! Note input from the host: engine-side data on the audio thread.
//!
//! Notes come from the host (CLAP note events, raw MIDI, Web MIDI and a
//! page's keyboard through the shared buffer's MIDI ring),
//! go straight to [`Engine::note`](crate::Engine::note), and never cross
//! the UI ↔ engine bridge or touch the model. The type lives here because
//! engine code depends only on this crate.

/// One note-input event, delivered to the engine at its sample offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteEvent {
    /// `velocity` in `0..=1`.
    On { channel: u8, key: u8, velocity: f32 },
    /// `velocity` in `0..=1`. A CLAP note choke arrives as an off with
    /// velocity 0.
    Off { channel: u8, key: u8, velocity: f32 },
    /// `value` in `-1..=1`, 0 at centre; `raw` is the 14-bit MIDI value
    /// (`0..=16383`, centre 8192).
    PitchBend { channel: u8, value: f32, raw: u16 },
    /// Any other MIDI message (control change, aftertouch, program change,
    /// system), passed through raw.
    Midi { data: [u8; 3] },
}

impl NoteEvent {
    /// Decode a MIDI 1.0 message. Note on with velocity 0 is a note off.
    /// Returns `None` only for a data byte where a status byte belongs.
    pub fn from_midi(data: [u8; 3]) -> Option<Self> {
        let status = data[0];
        if status < 0x80 {
            return None;
        }
        let channel = status & 0x0F;
        let (d1, d2) = (data[1] & 0x7F, data[2] & 0x7F);
        Some(match status & 0xF0 {
            0x90 if d2 > 0 => NoteEvent::On {
                channel,
                key: d1,
                velocity: f32::from(d2) / 127.0,
            },
            0x80 | 0x90 => NoteEvent::Off {
                channel,
                key: d1,
                velocity: f32::from(d2) / 127.0,
            },
            0xE0 => {
                let raw = u16::from(d1) | (u16::from(d2) << 7);
                NoteEvent::PitchBend {
                    channel,
                    value: bend_value(raw),
                    raw,
                }
            }
            _ => NoteEvent::Midi { data },
        })
    }
}

/// A 14-bit pitch bend as `-1..=1`: both extremes reach exactly ±1.
fn bend_value(raw: u16) -> f32 {
    let offset = f32::from(raw.min(16383)) - 8192.0;
    if offset >= 0.0 {
        offset / 8191.0
    } else {
        offset / 8192.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_and_off() {
        assert_eq!(
            NoteEvent::from_midi([0x93, 60, 127]),
            Some(NoteEvent::On {
                channel: 3,
                key: 60,
                velocity: 1.0
            })
        );
        assert_eq!(
            NoteEvent::from_midi([0x80, 60, 64]),
            Some(NoteEvent::Off {
                channel: 0,
                key: 60,
                velocity: 64.0 / 127.0
            })
        );
        assert_eq!(
            NoteEvent::from_midi([0x9F, 61, 0]),
            Some(NoteEvent::Off {
                channel: 15,
                key: 61,
                velocity: 0.0
            }),
            "velocity 0 is an off"
        );
    }

    #[test]
    fn pitch_bend_is_normalised_with_the_raw_value() {
        let bend = |lsb, msb| match NoteEvent::from_midi([0xE1, lsb, msb]) {
            Some(NoteEvent::PitchBend {
                channel: 1,
                value,
                raw,
            }) => (value, raw),
            other => panic!("{other:?}"),
        };
        assert_eq!(bend(0, 0x40), (0.0, 8192));
        assert_eq!(bend(0x7F, 0x7F), (1.0, 16383));
        assert_eq!(bend(0, 0), (-1.0, 0));
        let (v, raw) = bend(0, 0x60);
        assert_eq!(raw, 12288);
        assert!((v - 4096.0 / 8191.0).abs() < 1e-6);
    }

    #[test]
    fn everything_else_passes_through_raw() {
        for data in [[0xB2, 1, 100], [0xD0, 20, 0], [0xC5, 7, 0], [0xF8, 0, 0]] {
            assert_eq!(NoteEvent::from_midi(data), Some(NoteEvent::Midi { data }));
        }
        assert_eq!(NoteEvent::from_midi([0x40, 1, 2]), None);
    }
}
