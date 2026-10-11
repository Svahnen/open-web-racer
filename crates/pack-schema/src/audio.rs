//! Audio document: clips, engine sound layers and event cues.

use crate::buffer::{AccessorId, BufferBytes, BufferTable, ComponentType, ElementKind};
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioSet {
    #[serde(default)]
    pub data: BufferTable,
    pub clips: Vec<Clip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineSoundParams>,
    #[serde(default)]
    pub cues: Vec<EventCue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    /// Signed 16-bit PCM (`i16` accessor).
    Pcm16,
    /// 32-bit float in [-1, 1] (`f32` accessor).
    Float32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoopPoints {
    /// Frame indices; `start < end <= frames`.
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub format: SampleFormat,
    /// Interleaved samples, scalar accessor with `frames * channels` elements.
    pub samples: AccessorId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_points: Option<LoopPoints>,
}

/// Piecewise-linear curve; `points` are `[x, y]` with strictly increasing x.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Curve {
    /// Evaluate with clamping outside the point range.
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.points;
        if p.is_empty() {
            return 0.0;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        for w in p.windows(2) {
            if x <= w[1][0] {
                let t = (x - w[0][0]) / (w[1][0] - w[0][0]);
                return w[0][1] + t * (w[1][1] - w[0][1]);
            }
        }
        p[p.len() - 1][1]
    }

    fn validate(&self, path: &str, out: &mut Issues) {
        if self.points.is_empty() {
            out.add(path, "curve needs at least one point");
        }
        for (i, pt) in self.points.iter().enumerate() {
            out.finite(&format!("{path}.points[{i}]"), pt);
        }
        if self.points.windows(2).any(|w| w[1][0] <= w[0][0]) {
            out.add(path, "curve x values must be strictly increasing");
        }
    }
}

/// Which throttle state a layer is audible in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThrottleState {
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineLayer {
    pub clip: u32,
    pub throttle: ThrottleState,
    /// Engine speed at which the clip plays at its recorded pitch.
    pub reference_rpm: f32,
    /// Gain (0..1) as a function of engine rpm.
    pub gain_curve: Curve,
    /// Playback-rate multiplier as a function of engine rpm.
    pub pitch_curve: Curve,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineSoundParams {
    pub idle_rpm: f32,
    pub max_rpm: f32,
    pub layers: Vec<EngineLayer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CueKind {
    Skid,
    Impact,
    Horn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventCue {
    pub kind: CueKind,
    pub clip: u32,
    pub gain: f32,
    /// Playback-rate multiplier.
    #[serde(default = "one")]
    pub pitch: f32,
    /// Minimum seconds between retriggers.
    #[serde(default)]
    pub cooldown_s: f32,
}

fn one() -> f32 {
    1.0
}

impl AudioSet {
    pub fn validate(&self, path: &str, bytes: Option<&BufferBytes>, out: &mut Issues) {
        self.data.validate(&format!("{path}.data"), bytes, out);
        for (i, c) in self.clips.iter().enumerate() {
            let p = format!("{path}.clips[{i}]");
            if c.id.is_empty() {
                out.add(&format!("{p}.id"), "id must not be empty");
            }
            if !(1_000..=192_000).contains(&c.sample_rate) {
                out.add(
                    &format!("{p}.sample_rate"),
                    "sample rate must be within 1000..=192000 Hz",
                );
            }
            if c.channels == 0 || c.channels > 2 {
                out.add(&format!("{p}.channels"), "channels must be 1 or 2");
            }
            let comp = match c.format {
                SampleFormat::Pcm16 => ComponentType::I16,
                SampleFormat::Float32 => ComponentType::F32,
            };
            let ok = self.data.expect(
                c.samples,
                &format!("{p}.samples"),
                &[comp],
                ElementKind::Scalar,
                None,
                out,
            );
            if ok {
                let n = self.data.accessors[c.samples as usize].count;
                let ch = u32::from(c.channels.max(1));
                if n % ch != 0 {
                    out.add(
                        &format!("{p}.samples"),
                        format!("{n} samples is not a multiple of {ch} channels"),
                    );
                }
                let frames = n / ch;
                if let Some(l) = &c.loop_points {
                    if l.start_frame >= l.end_frame || l.end_frame > frames {
                        out.add(
                            &format!("{p}.loop_points"),
                            format!(
                                "loop {}..{} invalid for a clip of {frames} frames",
                                l.start_frame, l.end_frame
                            ),
                        );
                    }
                }
            }
        }
        if let Some(e) = &self.engine {
            let p = format!("{path}.engine");
            out.positive(&format!("{p}.idle_rpm"), e.idle_rpm);
            if e.max_rpm <= e.idle_rpm {
                out.add(&p, "max_rpm must exceed idle_rpm");
            }
            if e.layers.is_empty() {
                out.add(&format!("{p}.layers"), "engine needs at least one layer");
            }
            for (i, l) in e.layers.iter().enumerate() {
                let lp = format!("{p}.layers[{i}]");
                out.index(
                    &format!("{lp}.clip"),
                    l.clip as usize,
                    self.clips.len(),
                    "clip",
                );
                out.positive(&format!("{lp}.reference_rpm"), l.reference_rpm);
                l.gain_curve.validate(&format!("{lp}.gain_curve"), out);
                l.pitch_curve.validate(&format!("{lp}.pitch_curve"), out);
                if l.gain_curve
                    .points
                    .iter()
                    .any(|pt| !(0.0..=1.0).contains(&pt[1]))
                {
                    out.add(
                        &format!("{lp}.gain_curve"),
                        "gain values must be within 0..=1",
                    );
                }
                if l.pitch_curve.points.iter().any(|pt| pt[1] <= 0.0) {
                    out.add(&format!("{lp}.pitch_curve"), "pitch values must be > 0");
                }
            }
        }
        for (i, c) in self.cues.iter().enumerate() {
            let p = format!("{path}.cues[{i}]");
            out.index(
                &format!("{p}.clip"),
                c.clip as usize,
                self.clips.len(),
                "clip",
            );
            out.range(&format!("{p}.gain"), c.gain, 0.0, 1.0);
            out.positive(&format!("{p}.pitch"), c.pitch);
            out.range(&format!("{p}.cooldown_s"), c.cooldown_s, 0.0, 60.0);
        }
    }
}
