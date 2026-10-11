//! Computer-driver content: per-segment lanes, speed profiles, grid, per-car
//! limits and the AI parameter set (personalities, difficulty, catch-up, gains).
//!
//! Everything here is optional. A pack without any of it still runs computer
//! drivers: lanes and a speed profile are then derived from the centerline and
//! the engine's neutral defaults apply (see `docs/CONTENT_PACK.md`, section 9).

use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Number of lane slots per segment, indexed from the far left (0) to the far
/// right (13). Slot [`FIRST_RIGHT_LANE`] is the first lane right of the centre line.
pub const LANE_SLOTS: usize = 14;
/// Slot index of the first lane right of the centre line.
pub const FIRST_RIGHT_LANE: usize = 7;
/// Entries of a catch-up curve: progress differences in steps, `-10..=10`.
pub const GLUE_CURVE_LEN: usize = 21;
/// Number of skill levels (beginner, advanced).
pub const SKILL_LEVELS: usize = 2;
/// Maximum number of personalities in an [`AiSet`].
pub const MAX_PERSONALITIES: usize = 16;

/// Mask bit of a lane slot. Slots below -1 map to the top bit, slots above 14
/// are clamped, so `lane_mask_bit(s) = 1 << (14 - s)` for `-1 <= s <= 14`.
pub fn lane_mask_bit(slot: i32) -> u16 {
    if slot < -1 {
        0x8000
    } else {
        1u16 << (14 - slot.min(14))
    }
}

/// Lane layout of one centerline segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentLanes {
    /// Driving lanes left of the centre line (0..=7).
    pub left_count: u8,
    /// Driving lanes right of the centre line (0..=7).
    pub right_count: u8,
    /// Width of one left lane in metres (ignored when `left_count` is 0).
    pub left_width_m: f32,
    /// Width of one right lane in metres (ignored when `right_count` is 0).
    pub right_width_m: f32,
    /// Lane availability: bit `lane_mask_bit(slot)` is set when the slot
    /// exists on this segment, shoulder slots included. When absent, the driving
    /// lanes plus one shoulder slot on each side are available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_mask: Option<u16>,
    /// Preferred lane slot (0..=13) used as a small bias, or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_lane: Option<u8>,
}

impl SegmentLanes {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        if self.left_count > 7 {
            out.add(&format!("{path}.left_count"), "at most 7 lanes per side");
        }
        if self.right_count > 7 {
            out.add(&format!("{path}.right_count"), "at most 7 lanes per side");
        }
        if self.left_count == 0 && self.right_count == 0 {
            out.add(path, "segment needs at least one driving lane");
        }
        if self.left_count > 0 {
            out.range(
                &format!("{path}.left_width_m"),
                self.left_width_m,
                0.5,
                20.0,
            );
        }
        if self.right_count > 0 {
            out.range(
                &format!("{path}.right_width_m"),
                self.right_width_m,
                0.5,
                20.0,
            );
        }
        if let Some(mask) = self.availability_mask {
            if mask & 1 != 0 || mask & 0x8000 != 0 {
                out.add(
                    &format!("{path}.availability_mask"),
                    "only bits 1..=14 (slots 0..=13) may be set",
                );
            }
        }
        if let Some(l) = self.preferred_lane {
            if usize::from(l) >= LANE_SLOTS {
                out.add(&format!("{path}.preferred_lane"), "must be below 14");
            }
        }
    }
}

/// Target-speed profile along the centerline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeedProfile {
    /// Segments covered by one entry (at least 1; 2 is typical).
    pub segments_per_entry: u32,
    /// Target speed in whole metres per second, one entry per
    /// `segments_per_entry` segments in the driving direction.
    pub forward_mps: Vec<u16>,
    /// Profile for driving the track backwards (indexed in the reversed
    /// direction); falls back to the forward profile mirrored when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_mps: Option<Vec<u16>>,
}

/// Pace trim applied after a progress threshold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaceTrim {
    /// Cumulative segments driven after which `factor` applies.
    pub after_segments: u32,
    pub factor: f32,
}

/// Starting grid: positions are spaced `spacing_segments` apart in two columns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GridLayout {
    /// Segment of grid position 0 (the rearmost slot).
    pub start_segment: u32,
    /// Whole segments between consecutive grid positions (at least 1).
    pub spacing_segments: u32,
}

/// Computer-driver data of a track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackAi {
    /// Target speed used where no profile entry exists, metres per second.
    #[serde(default = "default_speed")]
    pub default_speed_mps: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_profile: Option<SpeedProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridLayout>,
    /// Late-race pace factor per skill level (beginner, advanced), applied
    /// after 100 cumulative segments. Missing means 1.0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub late_pace: Vec<f32>,
    /// Cumulative segments after which `late_pace` (and the beginner
    /// acceleration reduction) apply.
    #[serde(default = "default_late_after")]
    pub late_pace_after_segments: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace_trim: Option<PaceTrim>,
}

fn default_speed() -> f32 {
    50.0
}

fn default_late_after() -> u32 {
    100
}

impl TrackAi {
    pub fn validate(&self, path: &str, segment_count: usize, out: &mut Issues) {
        out.range(
            &format!("{path}.default_speed_mps"),
            self.default_speed_mps,
            1.0,
            300.0,
        );
        if let Some(p) = &self.speed_profile {
            let pp = format!("{path}.speed_profile");
            if p.segments_per_entry == 0 {
                out.add(&format!("{pp}.segments_per_entry"), "must be at least 1");
            } else {
                let need = segment_count.div_ceil(p.segments_per_entry as usize);
                if p.forward_mps.len() < need {
                    out.add(
                        &format!("{pp}.forward_mps"),
                        format!(
                            "{} entries cover fewer than the {segment_count} segments (need {need})",
                            p.forward_mps.len()
                        ),
                    );
                }
                if let Some(r) = &p.reverse_mps {
                    if r.len() < need {
                        out.add(
                            &format!("{pp}.reverse_mps"),
                            format!("{} entries; need {need}", r.len()),
                        );
                    }
                }
            }
            if p.forward_mps.contains(&0) {
                out.add(&format!("{pp}.forward_mps"), "entries must be positive");
            }
        }
        if let Some(g) = &self.grid {
            if g.spacing_segments == 0 {
                out.add(
                    &format!("{path}.grid.spacing_segments"),
                    "must be at least 1",
                );
            }
            if g.start_segment as usize >= segment_count.max(1) {
                out.add(
                    &format!("{path}.grid.start_segment"),
                    "must index a centerline segment",
                );
            }
        }
        if self.late_pace.len() > SKILL_LEVELS {
            out.add(
                &format!("{path}.late_pace"),
                "at most one entry per skill level",
            );
        }
        for (i, f) in self.late_pace.iter().enumerate() {
            out.range(&format!("{path}.late_pace[{i}]"), *f, 0.3, 2.0);
        }
        if let Some(t) = &self.pace_trim {
            out.range(&format!("{path}.pace_trim.factor"), t.factor, 0.3, 2.0);
        }
    }
}

/// Heading-error thresholds and slow-down of the misalignment speed term.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Misalignment {
    /// Heading error (degrees) below which there is no slow-down.
    pub start_deg: f32,
    /// Heading error (degrees) at and above which the slow-down is maximal.
    pub full_deg: f32,
    /// Maximum fractional slow-down (0..1).
    pub max_slowdown: f32,
}

/// Per-car limits for computer drivers (`VehicleDef.ai`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VehicleAi {
    /// Maximum speed the driver attempts, metres per second.
    pub speed_cap_mps: f32,
    /// Forward acceleration by whole metres per second of speed (index 0 =
    /// standstill). Speeds beyond the table use the last entry. Missing: the
    /// engine derives a simple falling curve from `speed_cap_mps`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceleration_mps2: Vec<f32>,
    /// Driver braking limit, metres per second squared.
    #[serde(default = "default_brake")]
    pub brake_mps2: f32,
    #[serde(default = "default_lateral")]
    pub lateral_accel_cap_mps2: f32,
    #[serde(default = "default_yaw")]
    pub yaw_accel_cap_rad_s2: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misalignment: Option<Misalignment>,
    /// Updates with zero acceleration after a gear change (0 = none).
    #[serde(default)]
    pub shift_pause_updates: u32,
}

fn default_brake() -> f32 {
    15.0
}
fn default_lateral() -> f32 {
    14.0
}
fn default_yaw() -> f32 {
    12.0
}

impl VehicleAi {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        out.range(
            &format!("{path}.speed_cap_mps"),
            self.speed_cap_mps,
            1.0,
            400.0,
        );
        out.range(&format!("{path}.brake_mps2"), self.brake_mps2, 1.0, 100.0);
        out.range(
            &format!("{path}.lateral_accel_cap_mps2"),
            self.lateral_accel_cap_mps2,
            1.0,
            100.0,
        );
        out.range(
            &format!("{path}.yaw_accel_cap_rad_s2"),
            self.yaw_accel_cap_rad_s2,
            0.5,
            100.0,
        );
        if self.acceleration_mps2.len() > 512 {
            out.add(&format!("{path}.acceleration_mps2"), "at most 512 entries");
        }
        for (i, a) in self.acceleration_mps2.iter().enumerate() {
            out.range(&format!("{path}.acceleration_mps2[{i}]"), *a, -100.0, 100.0);
        }
        if let Some(m) = &self.misalignment {
            let p = format!("{path}.misalignment");
            out.range(&format!("{p}.start_deg"), m.start_deg, 0.0, 180.0);
            out.range(&format!("{p}.full_deg"), m.full_deg, 0.0, 180.0);
            out.range(&format!("{p}.max_slowdown"), m.max_slowdown, 0.0, 1.0);
            if m.full_deg <= m.start_deg {
                out.add(&p, "full_deg must exceed start_deg");
            }
        }
    }
}

/// Aggression: triggered by repeated contact with a human, higher skill only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Aggression {
    /// Decision ticks of contact with the same human that trigger it.
    pub contact_ticks: u32,
    /// Decision ticks the aggressive mode lasts.
    pub duration_ticks: u32,
}

/// One driver personality.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Personality {
    pub name: String,
    /// Speed factor per skill level (beginner, advanced); 1.0 = unscaled.
    pub speed_factor: [f32; SKILL_LEVELS],
    /// Acceleration factor per skill level (used when catch-up is on).
    pub accel_factor: [f32; SKILL_LEVELS],
    /// Factor applied to the misalignment slow-down.
    pub handling_adjust: f32,
    /// Distance ahead (metres) within which cars in a lane count as blockers.
    pub blocker_look_ahead_m: f32,
    /// Speed above which lane changes are discouraged (m/s).
    pub lane_change_speed_limit_mps: f32,
    /// Steering look-ahead per m/s of speed (rows) and its clamps.
    pub steer_look_ahead_per_mps: f32,
    pub steer_look_ahead_min_rows: u32,
    pub steer_look_ahead_max_rows: u32,
    /// Lateral slack offsets (metres) the driver draws from.
    pub lane_slack_m: [f32; 4],
    /// The slack is re-drawn with probability `1 / n` per decision.
    pub slack_redraw_one_in: u32,
    /// Speed catch-up curve by progress difference (index 10 = level).
    pub speed_glue: Vec<f32>,
    /// Acceleration catch-up curve of the same shape.
    pub accel_glue: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggression: Option<Aggression>,
}

impl Personality {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        for (i, v) in self.speed_factor.iter().enumerate() {
            out.range(&format!("{path}.speed_factor[{i}]"), *v, 0.3, 2.0);
        }
        for (i, v) in self.accel_factor.iter().enumerate() {
            out.range(&format!("{path}.accel_factor[{i}]"), *v, 0.1, 3.0);
        }
        out.range(
            &format!("{path}.handling_adjust"),
            self.handling_adjust,
            0.0,
            4.0,
        );
        out.range(
            &format!("{path}.blocker_look_ahead_m"),
            self.blocker_look_ahead_m,
            1.0,
            500.0,
        );
        out.range(
            &format!("{path}.lane_change_speed_limit_mps"),
            self.lane_change_speed_limit_mps,
            1.0,
            400.0,
        );
        out.range(
            &format!("{path}.steer_look_ahead_per_mps"),
            self.steer_look_ahead_per_mps,
            0.0,
            10.0,
        );
        if self.steer_look_ahead_min_rows > self.steer_look_ahead_max_rows {
            out.add(path, "steer look-ahead min exceeds max");
        }
        out.finite(&format!("{path}.lane_slack_m"), &self.lane_slack_m);
        if self.slack_redraw_one_in == 0 {
            out.add(&format!("{path}.slack_redraw_one_in"), "must be at least 1");
        }
        for (name, curve) in [
            ("speed_glue", &self.speed_glue),
            ("accel_glue", &self.accel_glue),
        ] {
            if curve.len() != GLUE_CURVE_LEN {
                out.add(
                    &format!("{path}.{name}"),
                    format!("needs exactly {GLUE_CURVE_LEN} entries"),
                );
            }
            for (i, v) in curve.iter().enumerate() {
                out.range(&format!("{path}.{name}[{i}]"), *v, 0.1, 4.0);
            }
        }
    }
}

/// Catch-up (rubber-banding) parameters shared by all personalities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatchUp {
    /// Scale applied to the speed curve when the gap exceeds three steps
    /// (as the scale when the factor is above 1, as its reciprocal otherwise).
    pub speed_scale: f32,
    /// Same for the acceleration curve.
    pub accel_scale: f32,
    /// Segments per curve step.
    pub step_segments: u32,
    /// Fade-in thresholds per lap class (`[<=2 laps, <=4 laps, more]`): the
    /// deviation from 1.0 applies at 1/4, 1/2, 3/4 and in full as progress
    /// passes these cumulative segment counts.
    pub fade_in_segments: [[u32; 4]; 3],
    /// Range of the combined pace factor while a human is within
    /// `clamp_within_segments`.
    pub clamp_range: [f32; 2],
    pub clamp_within_segments: u32,
    /// Launch assist: the pace factor is raised by up to `start_boost_max`
    /// (a fraction, 1.0 = doubled) at progress 0, fading linearly to nothing
    /// at `start_boost_segments` cumulative segments. 0 segments disables it.
    #[serde(default)]
    pub start_boost_segments: u32,
    #[serde(default)]
    pub start_boost_max: f32,
}

impl CatchUp {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        out.range(&format!("{path}.speed_scale"), self.speed_scale, 0.5, 2.0);
        out.range(&format!("{path}.accel_scale"), self.accel_scale, 0.5, 2.0);
        if self.step_segments == 0 {
            out.add(&format!("{path}.step_segments"), "must be at least 1");
        }
        for (c, row) in self.fade_in_segments.iter().enumerate() {
            if row.windows(2).any(|w| w[0] > w[1]) {
                out.add(
                    &format!("{path}.fade_in_segments[{c}]"),
                    "thresholds must be non-decreasing",
                );
            }
        }
        let [lo, hi] = self.clamp_range;
        out.range(&format!("{path}.clamp_range[0]"), lo, 0.1, 1.0);
        out.range(&format!("{path}.clamp_range[1]"), hi, 1.0, 4.0);
        out.range(
            &format!("{path}.start_boost_max"),
            self.start_boost_max,
            0.0,
            2.0,
        );
    }
}

/// Weights of the lane-choice penalty terms (all non-negative).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaneWeights {
    pub absent_now: f32,
    pub absent_ahead: f32,
    pub absent_near: f32,
    pub shoulder: f32,
    pub edge: f32,
    pub adjacent_car: f32,
    /// Per m/s of speed excess over a slower car ahead in the lane.
    pub blocker_speed: f32,
    /// Per m/s of closing speed to an obstacle ahead in the lane.
    pub obstacle: f32,
    /// Bonus (subtracted) for the preferred lane.
    pub hint: f32,
    /// Bonus for blocking a human close behind (advanced skill only).
    pub block_human: f32,
}

impl LaneWeights {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        for (n, v) in [
            ("absent_now", self.absent_now),
            ("absent_ahead", self.absent_ahead),
            ("absent_near", self.absent_near),
            ("shoulder", self.shoulder),
            ("edge", self.edge),
            ("adjacent_car", self.adjacent_car),
            ("blocker_speed", self.blocker_speed),
            ("obstacle", self.obstacle),
            ("hint", self.hint),
            ("block_human", self.block_human),
        ] {
            out.range(&format!("{path}.{n}"), v, 0.0, 1.0e6);
        }
    }
}

/// One steering gain set (normal or recovery regime).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GainSet {
    /// Lateral position error (m) to lateral velocity (m/s).
    pub lateral_pos_to_vel: f32,
    pub max_lateral_vel_mps: f32,
    /// Lateral velocity error to lateral acceleration (1/s).
    pub lateral_vel_to_accel: f32,
    /// Heading error (rad) to yaw rate (rad/s).
    pub heading_to_yaw_rate: f32,
    pub max_yaw_rate_rad_s: f32,
    /// Yaw-rate error to yaw acceleration (1/s).
    pub yaw_rate_to_accel: f32,
    /// Below this speed the lateral and yaw targets are scaled down.
    pub low_speed_mps: f32,
    pub low_speed_lateral_factor: f32,
    pub low_speed_yaw_factor: f32,
}

impl GainSet {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        for (n, v, lo, hi) in [
            ("lateral_pos_to_vel", self.lateral_pos_to_vel, 0.0, 100.0),
            ("max_lateral_vel_mps", self.max_lateral_vel_mps, 0.0, 100.0),
            (
                "lateral_vel_to_accel",
                self.lateral_vel_to_accel,
                0.0,
                100.0,
            ),
            ("heading_to_yaw_rate", self.heading_to_yaw_rate, 0.0, 100.0),
            ("max_yaw_rate_rad_s", self.max_yaw_rate_rad_s, 0.0, 50.0),
            ("yaw_rate_to_accel", self.yaw_rate_to_accel, 0.0, 100.0),
            ("low_speed_mps", self.low_speed_mps, 0.0, 100.0),
            (
                "low_speed_lateral_factor",
                self.low_speed_lateral_factor,
                0.0,
                1.0,
            ),
            ("low_speed_yaw_factor", self.low_speed_yaw_factor, 0.0, 1.0),
        ] {
            out.range(&format!("{path}.{n}"), v, lo, hi);
        }
    }
}

/// Normal and recovery gain sets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gains {
    pub normal: GainSet,
    pub recovery: GainSet,
}

/// An AI parameter set document (`ai/<n>.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiSet {
    pub id: String,
    pub name: String,
    /// Slot `k` (1-based among opponents) uses personality `(k - 1) % len`.
    pub personalities: Vec<Personality>,
    pub catch_up: CatchUp,
    pub lane_weights: LaneWeights,
    pub gains: Gains,
}

impl AiSet {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        if self.id.is_empty() {
            out.add(&format!("{path}.id"), "id must not be empty");
        }
        if self.personalities.is_empty() || self.personalities.len() > MAX_PERSONALITIES {
            out.add(
                &format!("{path}.personalities"),
                format!("needs 1..={MAX_PERSONALITIES} personalities"),
            );
        }
        for (i, p) in self.personalities.iter().enumerate() {
            p.validate(&format!("{path}.personalities[{i}]"), out);
        }
        self.catch_up.validate(&format!("{path}.catch_up"), out);
        self.lane_weights
            .validate(&format!("{path}.lane_weights"), out);
        self.gains
            .normal
            .validate(&format!("{path}.gains.normal"), out);
        self.gains
            .recovery
            .validate(&format!("{path}.gains.recovery"), out);
    }
}
