//! What the scope and meter controls show (design/engine-bridge.md): the engine's
//! output and meters, read from the link's telemetry each tick, kept
//! with a generation each so their widgets know when to repaint.

use silkscreen_bridge::ParamIndex;
use silkscreen_bridge::telemetry::{ENGINE_METERS, ENGINE_VALUES, SCOPE_CAPACITY};
use silkscreen_core::telemetry::{
    Ballistics, MeterReading, MeterStyle, SCOPE_WINDOW, ScopeStyle, trigger,
};
use silkscreen_core::{ControlId, Layout};

use super::EditorState;

/// What a meter control shows (design/engine-bridge.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeterSource {
    /// The plugin's output, as the host taps it.
    #[default]
    Output,
    /// Meter `n` the engine reports from inside itself (`Engine::meters`).
    Engine(usize),
}

/// One meter control: where it reads, and its latest reading.
#[derive(Debug)]
pub(super) struct MeterFeed {
    control: ControlId,
    source: MeterSource,
    ballistics: Ballistics,
    reading: MeterReading,
    /// Counts readings, so widgets know when to repaint.
    generation: u64,
}

/// What the scope and meter controls draw, and their styles. Their
/// widgets copy it when its generation moves on.
#[derive(Debug)]
pub(super) struct Live {
    pub scope_style: ScopeStyle,
    pub meter_style: MeterStyle,
    /// The triggered window the scope draws.
    trace: Vec<f32>,
    /// Counts traces.
    scope_generation: u64,
    meters: Vec<MeterFeed>,
    /// The scope ring's latest copy, and a count of the copies.
    samples: Box<[f32; SCOPE_CAPACITY]>,
    samples_generation: u64,
    /// When the scope last took new samples.
    scope_ms: Option<f64>,
    /// Each engine meter's latest stereo peak, taken every tick whether
    /// or not a meter shows it (a level key reads it too).
    engine: [[f32; 2]; ENGINE_METERS],
    /// The engine's own values (`Engine::values`), and a count of
    /// the ticks that changed them.
    values: [f32; ENGINE_VALUES],
    values_generation: u64,
    /// The engine's latest frame (`Engine::frame`), and a count of the
    /// frames taken.
    frame: Vec<f32>,
    frame_generation: u64,
}

impl Live {
    pub(super) fn new(layout: &Layout) -> Self {
        Self {
            scope_style: ScopeStyle::default(),
            meter_style: MeterStyle::default(),
            trace: Vec::new(),
            scope_generation: 0,
            meters: layout
                .controls
                .iter()
                .filter(|c| c.kind == silkscreen_core::ControlKind::Meter)
                .map(|c| MeterFeed {
                    control: c.id,
                    source: MeterSource::Output,
                    ballistics: Ballistics::default(),
                    reading: MeterReading::default(),
                    generation: 0,
                })
                .collect(),
            samples: Box::new([0.0; SCOPE_CAPACITY]),
            samples_generation: 0,
            scope_ms: None,
            engine: [[0.0; 2]; ENGINE_METERS],
            values: [0.0; ENGINE_VALUES],
            values_generation: 0,
            frame: Vec::new(),
            frame_generation: 0,
        }
    }

    fn meter(&self, control: ControlId) -> Option<&MeterFeed> {
        self.meters.iter().find(|m| m.control == control)
    }
}

impl EditorState {
    /// How scope controls look. Set before a view is made: a view's
    /// widgets take the style they're made with.
    pub fn set_scope_style(&mut self, style: ScopeStyle) {
        self.live.scope_style = style;
    }

    /// How meter controls look and move. Set before a view is made.
    pub fn set_meter_style(&mut self, style: MeterStyle) {
        self.live.meter_style = style;
    }

    /// Point meter `control` at `source` (design/engine-bridge.md). Meters show the
    /// output until told otherwise.
    ///
    /// # Panics
    /// If `control` isn't a meter.
    pub fn set_meter_source(&mut self, control: ControlId, source: MeterSource) {
        let meter = self
            .live
            .meters
            .iter_mut()
            .find(|m| m.control == control)
            .unwrap_or_else(|| panic!("control {control:?} is not a meter"));
        meter.source = source;
        meter.ballistics = Ballistics::default();
    }

    /// Read the engine's output from the link, if it carries telemetry,
    /// into what the scope and meter controls draw (design/engine-bridge.md).
    pub(super) fn pump_telemetry(&mut self, now_ms: f64) {
        let live = &mut self.live;
        let Some(reader) = self.link.telemetry() else {
            return;
        };
        let due = live
            .scope_ms
            .is_none_or(|t| now_ms - t >= live.scope_style.refresh_ms);
        if due && reader.scope(&mut live.samples) {
            live.scope_ms = Some(now_ms);
            live.samples_generation += 1;
            let start = trigger(&live.samples[..], SCOPE_WINDOW, live.scope_style.hysteresis);
            let window = &live.samples[start..start + SCOPE_WINDOW];
            // Silence (or anything unchanged) draws the same trace:
            // don't repaint it.
            if live.trace.as_slice() != window {
                live.trace.clear();
                live.trace.extend_from_slice(window);
                live.scope_generation += 1;
            }
        }
        let mut values = [0.0; ENGINE_VALUES];
        reader.engine_values(&mut values);
        if values.map(f32::to_bits) != live.values.map(f32::to_bits) {
            live.values = values;
            live.values_generation += 1;
        }
        if reader.poll_frame() {
            live.frame.clear();
            live.frame.extend_from_slice(reader.frame());
            live.frame_generation += 1;
        }
        // Each source is taken once a tick, then shared by its meters.
        let output = reader.take_peaks();
        for (n, levels) in live.engine.iter_mut().enumerate() {
            *levels = reader.take_engine_peaks(n);
        }
        for meter in &mut live.meters {
            let (channels, levels) = match meter.source {
                MeterSource::Output => (output.channels, output.levels),
                MeterSource::Engine(n) => (2, live.engine.get(n).copied().unwrap_or_default()),
            };
            if meter
                .ballistics
                .update(channels, levels, now_ms, &live.meter_style)
            {
                meter.reading = meter.ballistics.reading();
                meter.generation += 1;
            }
        }
    }

    /// The peak below which [`match_level`](Self::match_level) declines:
    /// −60 dBFS.
    pub const MATCH_LEVEL_FLOOR: f32 = 1e-3;

    /// Set `param`, a linear gain, so engine meter `meter`'s latest peak
    /// (measured before that gain) lands at `target_db` dBFS, held to the
    /// param's range, as one undo step: one press, nothing acting after
    /// it. Declines (returns false) under
    /// [`MATCH_LEVEL_FLOOR`](Self::MATCH_LEVEL_FLOOR) or without a reading.
    pub fn match_level(&mut self, meter: usize, param: ParamIndex, target_db: f32) -> bool {
        let peak = self.live.engine.get(meter).map_or(0.0, |[l, r]| l.max(*r));
        // NaN declines too.
        if peak.is_nan() || peak <= Self::MATCH_LEVEL_FLOOR {
            return false;
        }
        let Some(spec) = self.model.specs().get(param.index()) else {
            return false;
        };
        let gain = spec.clamp(10f32.powf(target_db / 20.0) / peak);
        self.edit(|e| {
            e.begin(param);
            e.set(param, gain);
            e.end(param);
        });
        true
    }

    /// The scope's latest trace and its generation, which moves on with
    /// each new trace: for a plugin's own scope widget.
    pub fn scope_trace(&self) -> (&[f32], u64) {
        (&self.live.trace, self.live.scope_generation)
    }

    /// The scope ring's latest copy, oldest first, untriggered, and its
    /// generation, which moves on with each copy: for a plugin scope that
    /// triggers and spans its window its own way.
    pub fn scope_samples(&self) -> (&[f32; SCOPE_CAPACITY], u64) {
        (&self.live.samples, self.live.samples_generation)
    }

    /// The engine's values (`Engine::values`, design/engine-bridge.md) as of the
    /// last tick, and their generation, which moves on whenever any
    /// changes: what a plugin's canvas shows of the engine.
    pub fn engine_values(&self) -> (&[f32; ENGINE_VALUES], u64) {
        (&self.live.values, self.live.values_generation)
    }

    /// The engine's latest frame (`Engine::frame`, design/engine-bridge.md),
    /// whole, as of the last tick, and its generation, which moves on with
    /// each new frame: what a plugin's canvas draws of many engine values
    /// that belong together. Empty until the engine publishes one.
    pub fn engine_frame(&self) -> (&[f32], u64) {
        (&self.live.frame, self.live.frame_generation)
    }

    /// Meter `control`'s latest reading and its generation.
    pub fn meter_reading(&self, control: ControlId) -> Option<(MeterReading, u64)> {
        self.live.meter(control).map(|m| (m.reading, m.generation))
    }
}
