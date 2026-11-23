//! The CLAP note-ports extension and host note input: CLAP note events and
//! raw MIDI, decoded for [`Engine::note`](silkscreen::bridge::Engine::note).

use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePortsImpl,
};
use clack_plugin::events::Match;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use silkscreen::bridge::NoteEvent;

use crate::{MainThread, SilkscreenPlugin};

impl<P: SilkscreenPlugin> PluginNotePortsImpl for MainThread<'_, P> {
    fn count(&mut self, is_input: bool) -> u32 {
        u32::from(is_input && P::NOTE_INPUT)
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if index != 0 || !is_input || !P::NOTE_INPUT {
            return;
        }
        writer.set(&NotePortInfo {
            id: ClapId::new(0),
            name: b"notes",
            supported_dialects: NoteDialects::CLAP | NoteDialects::MIDI,
            preferred_dialect: Some(NoteDialect::Clap),
        });
    }
}

/// `Some(n)` for a specific value below `limit`; wildcards and out-of-range
/// values are `None`.
fn specific(m: Match<u16>, limit: u16) -> Option<u8> {
    match m {
        Match::Specific(v) if v < limit => Some(v as u8),
        _ => None,
    }
}

/// Whether an event for `port` is for our single note port.
fn our_port(port: Match<u16>) -> bool {
    matches!(port, Match::All | Match::Specific(0))
}

/// A host input event as a note event, if it is one for our port.
/// Note events addressed to all keys or channels (wildcards) are ignored.
/// Never allocates.
pub(crate) fn note_event(event: &UnknownEvent) -> Option<NoteEvent> {
    match event.as_core_event()? {
        CoreEventSpace::NoteOn(e) if our_port(e.port_index()) => Some(NoteEvent::On {
            channel: specific(e.channel(), 16)?,
            key: specific(e.key(), 128)?,
            velocity: e.velocity().clamp(0.0, 1.0) as f32,
        }),
        CoreEventSpace::NoteOff(e) if our_port(e.port_index()) => Some(NoteEvent::Off {
            channel: specific(e.channel(), 16)?,
            key: specific(e.key(), 128)?,
            velocity: e.velocity().clamp(0.0, 1.0) as f32,
        }),
        CoreEventSpace::NoteChoke(e) if our_port(e.port_index()) => Some(NoteEvent::Off {
            channel: specific(e.channel(), 16)?,
            key: specific(e.key(), 128)?,
            velocity: 0.0,
        }),
        CoreEventSpace::Midi(e) if e.port_index() == 0 => NoteEvent::from_midi(e.data()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clack_plugin::events::Pckn;
    use clack_plugin::events::event_types::{MidiEvent, NoteChokeEvent, NoteOffEvent, NoteOnEvent};
    use clack_plugin::events::io::EventBuffer;

    fn decode(push: impl FnOnce(&mut EventBuffer)) -> Option<NoteEvent> {
        let mut buf = EventBuffer::new();
        push(&mut buf);
        note_event(buf.iter().next().unwrap())
    }

    #[test]
    fn clap_notes() {
        let pckn = Pckn::new(0u16, 2u16, 64u16, Match::All);
        assert_eq!(
            decode(|b| b.push(&NoteOnEvent::new(5, pckn, 0.5))),
            Some(NoteEvent::On {
                channel: 2,
                key: 64,
                velocity: 0.5
            })
        );
        assert_eq!(
            decode(|b| b.push(&NoteOffEvent::new(5, pckn, 0.25))),
            Some(NoteEvent::Off {
                channel: 2,
                key: 64,
                velocity: 0.25
            })
        );
        assert_eq!(
            decode(|b| b.push(&NoteChokeEvent::new(5, pckn))),
            Some(NoteEvent::Off {
                channel: 2,
                key: 64,
                velocity: 0.0
            })
        );
    }

    #[test]
    fn wildcards_and_other_ports_are_ignored() {
        let all_keys = Pckn::new(0u16, 0u16, Match::All, Match::All);
        assert_eq!(
            decode(|b| b.push(&NoteOffEvent::new(0, all_keys, 0.0))),
            None
        );
        let port1 = Pckn::new(1u16, 0u16, 60u16, Match::All);
        assert_eq!(decode(|b| b.push(&NoteOnEvent::new(0, port1, 1.0))), None);
        assert_eq!(
            decode(|b| b.push(&MidiEvent::new(0, 1, [0x90, 60, 100]))),
            None
        );
    }

    #[test]
    fn midi_goes_through_the_midi_decoder() {
        assert_eq!(
            decode(|b| b.push(&MidiEvent::new(0, 0, [0xE0, 0, 0x40]))),
            Some(NoteEvent::PitchBend {
                channel: 0,
                value: 0.0,
                raw: 8192
            })
        );
        assert_eq!(
            decode(|b| b.push(&MidiEvent::new(0, 0, [0xB0, 74, 3]))),
            Some(NoteEvent::Midi {
                data: [0xB0, 74, 3]
            })
        );
    }
}
