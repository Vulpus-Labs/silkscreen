//! Audio I/O for standalone silkscreen apps: a cpal output stream
//! whose callback drives an [`Engine`] through the native bridge's
//! [`EngineEnd`], following Patches' `patches-cpal`.
//!
//! The output stream drives everything. When the engine wants the input
//! device, a second stream pushes its frames into a ring that the output
//! callback drains. [`Runner`] is the callback's work apart from any
//! device, so it's tested without one.
//!

mod runner;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use silkscreen_bridge::{Engine, EngineEnd};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// The runner's own surface, public for `tests/no_alloc.rs`; apps use
// `start` and `Source`.
#[doc(hidden)]
pub use runner::{CHUNK, Feed, Runner};

/// Stereo frames the input ring holds: about a second at 48 kHz.
const INPUT_RING: usize = 65_536;

/// What feeds the engine's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Silence: for instruments.
    Silence,
    /// A quiet 220 Hz sawtooth, for hearing an effect safely.
    TestTone,
    /// The default input device. Through speakers, a microphone feeds back.
    Input,
}

/// Why audio couldn't start.
#[derive(Debug)]
pub enum AudioError {
    NoOutputDevice,
    NoInputDevice,
    /// The input device runs at another rate than the output.
    SampleRateMismatch {
        input: u32,
        output: u32,
    },
    UnsupportedSampleFormat(SampleFormat),
    /// cpal's own error, with what was being done.
    Device(&'static str, String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::NoOutputDevice => write!(f, "no audio output device"),
            AudioError::NoInputDevice => write!(f, "no audio input device"),
            AudioError::SampleRateMismatch { input, output } => write!(
                f,
                "the input device runs at {input} Hz and the output at {output} Hz; set them to the same rate (Audio MIDI Setup on macOS)"
            ),
            AudioError::UnsupportedSampleFormat(format) => {
                write!(f, "the device's sample format {format} isn't supported")
            }
            AudioError::Device(doing, e) => write!(f, "{doing}: {e}"),
        }
    }
}

impl std::error::Error for AudioError {}

fn device_error(doing: &'static str) -> impl Fn(&dyn std::fmt::Display) -> AudioError {
    move |e| AudioError::Device(doing, e.to_string())
}

/// Running audio. Dropping it stops the streams.
pub struct Audio {
    _output: Stream,
    _input: Option<Stream>,
    sample_rate: u32,
    failed: Arc<AtomicBool>,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

impl Audio {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether the engine panicked; audio has been silent since.
    pub fn failed(&self) -> bool {
        self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Open the default output device, build the engine at its sample rate
/// with `make`, and start rendering it through `end`.
pub fn start<E: Engine>(
    make: impl FnOnce(f32) -> E,
    end: EngineEnd,
    source: Source,
) -> Result<Audio, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(AudioError::NoOutputDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|e| device_error("reading the output device's configuration")(&e))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let sample_rate = config.sample_rate.0;
    let channels = usize::from(config.channels);

    let (feed, input) = match source {
        Source::Silence => (Feed::Silence, None),
        Source::TestTone => (Feed::TestTone, None),
        Source::Input => {
            let (stream, rx) = open_input(&host, sample_rate)?;
            (Feed::Input(rx), Some(stream))
        }
    };
    let runner = Runner::new(make(sample_rate as f32), end, feed, sample_rate as f32);
    let failed = runner.failed();
    let output = match format {
        SampleFormat::F32 => output_stream::<f32, E>(&device, &config, channels, runner),
        SampleFormat::I16 => output_stream::<i16, E>(&device, &config, channels, runner),
        SampleFormat::U16 => output_stream::<u16, E>(&device, &config, channels, runner),
        other => Err(AudioError::UnsupportedSampleFormat(other)),
    }?;
    if let Some(input) = &input {
        input
            .play()
            .map_err(|e| device_error("starting the input stream")(&e))?;
    }
    output
        .play()
        .map_err(|e| device_error("starting the output stream")(&e))?;
    Ok(Audio {
        _output: output,
        _input: input,
        sample_rate,
        failed,
    })
}

fn output_stream<T, E>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    mut runner: Runner<E>,
) -> Result<Stream, AudioError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
    E: Engine,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| runner.fill(data, channels),
            |err| eprintln!("silkscreen: audio output stream error: {err}"),
            None,
        )
        .map_err(|e| device_error("opening the output stream")(&e))
}

/// The default input device's stream, pushing stereo frames (mono doubled)
/// into a ring, and the ring's consumer for the output callback.
fn open_input(
    host: &cpal::Host,
    output_rate: u32,
) -> Result<(Stream, rtrb::Consumer<[f32; 2]>), AudioError> {
    let device = host
        .default_input_device()
        .ok_or(AudioError::NoInputDevice)?;
    let supported = device
        .default_input_config()
        .map_err(|e| device_error("reading the input device's configuration")(&e))?;
    let input_rate = supported.sample_rate().0;
    if input_rate != output_rate {
        return Err(AudioError::SampleRateMismatch {
            input: input_rate,
            output: output_rate,
        });
    }
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let (tx, rx) = rtrb::RingBuffer::new(INPUT_RING);
    let stream = match format {
        SampleFormat::F32 => input_stream::<f32>(&device, &config, channels, tx),
        SampleFormat::I16 => input_stream::<i16>(&device, &config, channels, tx),
        SampleFormat::U16 => input_stream::<u16>(&device, &config, channels, tx),
        other => Err(AudioError::UnsupportedSampleFormat(other)),
    }?;
    Ok((stream, rx))
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    mut tx: rtrb::Producer<[f32; 2]>,
) -> Result<Stream, AudioError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = channels.max(1);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks_exact(channels) {
                    let left: f32 = cpal::FromSample::from_sample_(frame[0]);
                    let right: f32 = frame
                        .get(1)
                        .map_or(left, |&s| cpal::FromSample::from_sample_(s));
                    // A full ring drops frames: the output has stalled, and
                    // blocking here would only stall the input too.
                    let _ = tx.push([left, right]);
                }
            },
            |err| eprintln!("silkscreen: audio input stream error: {err}"),
            None,
        )
        .map_err(|e| device_error("opening the input stream")(&e))
}
