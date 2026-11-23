//! CLAP transport → [`Transport`], with per-field fallbacks.

use clack_plugin::events::event_types::{TransportEvent, TransportFlags};
use silkscreen::bridge::Transport;

/// The engine's view of the host transport. Fields the host doesn't
/// supply (or nonsense values) keep [`Transport::default`]'s.
pub(crate) fn from_clap(event: Option<&TransportEvent>) -> Transport {
    let mut t = Transport::default();
    let Some(e) = event else {
        return t;
    };
    if e.flags.contains(TransportFlags::HAS_TEMPO) && e.tempo.is_finite() && e.tempo > 0.0 {
        t.tempo = e.tempo;
    }
    t.playing = e.flags.contains(TransportFlags::IS_PLAYING);
    if e.flags.contains(TransportFlags::HAS_BEATS_TIMELINE) {
        t.beats = e.song_pos_beats.to_float();
    }
    if e.flags.contains(TransportFlags::HAS_TIME_SIGNATURE)
        && e.time_signature_numerator > 0
        && e.time_signature_denominator > 0
    {
        t.time_signature = (e.time_signature_numerator, e.time_signature_denominator);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use clack_plugin::events::{EventFlags, EventHeader};
    use clack_plugin::utils::{BeatTime, SecondsTime};

    fn fake(flags: TransportFlags, tempo: f64, beats: f64, sig: (u16, u16)) -> TransportEvent {
        TransportEvent {
            header: EventHeader::new_core(0, EventFlags::empty()),
            flags,
            song_pos_beats: BeatTime::from_float(beats),
            song_pos_seconds: SecondsTime::from_float(0.0),
            tempo,
            tempo_inc: 0.0,
            loop_start_beats: BeatTime::from_float(0.0),
            loop_end_beats: BeatTime::from_float(0.0),
            loop_start_seconds: SecondsTime::from_float(0.0),
            loop_end_seconds: SecondsTime::from_float(0.0),
            bar_start: BeatTime::from_float(0.0),
            bar_number: 0,
            time_signature_numerator: sig.0,
            time_signature_denominator: sig.1,
        }
    }

    #[test]
    fn no_transport_is_120_stopped() {
        assert_eq!(from_clap(None), Transport::default());
        let d = Transport::default();
        assert_eq!((d.tempo, d.playing, d.beats), (120.0, false, 0.0));
    }

    #[test]
    fn a_full_transport_is_read() {
        let flags = TransportFlags::HAS_TEMPO
            | TransportFlags::HAS_BEATS_TIMELINE
            | TransportFlags::HAS_TIME_SIGNATURE
            | TransportFlags::IS_PLAYING;
        let t = from_clap(Some(&fake(flags, 93.5, 17.25, (7, 8))));
        assert_eq!(
            t,
            Transport {
                tempo: 93.5,
                playing: true,
                beats: 17.25,
                time_signature: (7, 8),
            }
        );
    }

    #[test]
    fn missing_or_bad_fields_fall_back() {
        // Flags say nothing but "playing": the values are ignored.
        let t = from_clap(Some(&fake(TransportFlags::IS_PLAYING, 200.0, 5.0, (3, 4))));
        assert_eq!(
            t,
            Transport {
                playing: true,
                ..Transport::default()
            }
        );
        let all = TransportFlags::HAS_TEMPO | TransportFlags::HAS_TIME_SIGNATURE;
        let t = from_clap(Some(&fake(all, 0.0, 0.0, (0, 4))));
        assert_eq!(t, Transport::default());
        let t = from_clap(Some(&fake(all, f64::NAN, 0.0, (4, 0))));
        assert_eq!(t, Transport::default());
    }
}
