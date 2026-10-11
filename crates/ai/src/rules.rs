//! Integer rules of the driver: lane arithmetic, lane choice, the target-speed
//! composition, catch-up, difficulty and the longitudinal limits.
//!
//! Every function works on Q16 fixed-point integers (see [`crate::q16`]) so
//! the arithmetic is identical on every machine; the runtime driver converts
//! its float state at the boundary. The constants that the content pack may
//! tune (step width, clamp range, boost window, late-race threshold) are
//! parameters; [`RuleConstants::DEFAULT`] holds the neutral defaults that the
//! reference vectors use.

use crate::q16::{ONE, q, recip, wrap16, wrap32};

/// Number of lane slots.
pub const LANES: i32 = 14;
/// Entries in a catch-up curve.
pub const GLUE_LEN: usize = 21;

/// Tunable constants of the rules (all in segments / Q16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleConstants {
    /// Segments per catch-up curve step.
    pub step: i32,
    /// Gaps beyond this many segments scale the curve (two steps).
    pub far: i32,
    /// A human within this many segments clamps the pace factor.
    pub near_human: i32,
    /// Clamp range of the pace factor near a human.
    pub clamp_lo: i32,
    pub clamp_hi: i32,
    /// Progress below which the launch assist applies.
    pub boost_window: i32,
    /// Q16 increment of the launch factor per segment below the window.
    pub boost_per_segment: i32,
    /// Progress above which the late-race factors apply.
    pub late_after: i32,
}

impl RuleConstants {
    pub const DEFAULT: Self = Self {
        step: 15,
        far: 30,
        near_human: 20,
        clamp_lo: 39_321,
        clamp_hi: 85_196,
        boost_window: 65,
        boost_per_segment: 1_008,
        late_after: 100,
    };
}

impl Default for RuleConstants {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Signed distance `a - b` wrapped onto a circuit of `rows` segments.
pub fn row_delta(rows: i32, a: i32, b: i32) -> i32 {
    let d = a - b;
    let half = rows / 2;
    if d > half {
        d - rows
    } else if d < -half {
        d + rows
    } else {
        d
    }
}

/// Mask bit of a lane slot (see `racer_pack_schema::ai::lane_mask_bit`).
pub fn lane_bit(lane: i32) -> i32 {
    i32::from(racer_pack_schema::ai::lane_mask_bit(lane))
}

/// Lane slot of a lateral offset `x` (Q16 metres, negative = left) given the
/// reciprocal lane widths (Q16, `1 / width_in_metres`).
pub fn lane_of_offset(x: i32, inv_width_left: i32, inv_width_right: i32) -> i32 {
    let lane = if x < 0 {
        6 + q(x, inv_width_left) / ONE
    } else {
        7 + q(x, inv_width_right) / ONE
    };
    lane.clamp(0, LANES - 1)
}

/// Choose among three lane scores (higher is better). Ties keep the lane;
/// at most one lane is moved per call.
pub fn pick_lane(lane: i32, left: i64, stay: i64, right: i64) -> i32 {
    let mut chosen = lane;
    let mut best = stay;
    if stay < left {
        chosen = lane - 1;
        best = left;
    }
    if best < right {
        chosen = lane + 1;
    }
    chosen.clamp(0, LANES - 1)
}

/// Fade a catch-up factor in as progress passes the four thresholds.
pub fn fade_in(thresholds: &[i32; 4], progress: i32, g: i32) -> i32 {
    let k = g - ONE;
    if progress > thresholds[3] {
        g
    } else if progress > thresholds[2] {
        ONE + q(k, 49_152)
    } else if progress > thresholds[1] {
        ONE + q(k, 32_768)
    } else if progress > thresholds[0] {
        ONE + q(k, 16_384)
    } else {
        ONE
    }
}

/// Slow-down for a heading error (units: any consistent angle unit).
pub fn misalignment(heading_error: i32, hi: i32, lo: i32, slope: i32, handling: i32) -> i32 {
    let a = heading_error.abs().clamp(lo, hi);
    let ramp = wrap32(i64::from(a - lo) * i64::from(ONE));
    ONE - q(q(ramp, slope), handling)
}

/// Clamp the pace factor while any human is within `near_human` segments.
pub fn proximity_clamp(
    c: &RuleConstants,
    factor: i32,
    rows: i32,
    row: i32,
    human_rows: &[i32],
) -> i32 {
    let near = human_rows
        .iter()
        .any(|&h| row_delta(rows, h, row).abs() <= c.near_human);
    if near {
        factor.clamp(c.clamp_lo, c.clamp_hi)
    } else {
        factor
    }
}

/// Difficulty factor.
pub fn skill_factor(c: &RuleConstants, use_tier: bool, tier: i32, late: i32, progress: i32) -> i32 {
    let mut f = ONE;
    if use_tier {
        f = tier;
    }
    if progress > c.late_after {
        f = q(f, late);
    }
    f
}

/// Catch-up curve index for a progress difference (0..=20, 10 = level).
pub fn glue_index(c: &RuleConstants, p: i32, lead_p: i32) -> usize {
    let d = wrap16(p - lead_p);
    ((d / c.step) + 10).clamp(0, GLUE_LEN as i32 - 1) as usize
}

/// Inputs of [`speed_catchup`].
#[derive(Clone, Copy, Debug)]
pub struct CatchUpInput<'a> {
    pub progress: i32,
    pub leader_progress: i32,
    pub direction: i32,
    pub leader_speed: i32,
    pub leader_hold: i32,
    pub speed_glue: &'a [i32; GLUE_LEN],
    pub g_speed: i32,
    pub fade: [i32; 4],
}

/// Speed catch-up factor relative to the leading human.
pub fn speed_catchup(c: &RuleConstants, i: &CatchUpInput) -> i32 {
    let p = i.progress;
    let d = wrap16(p - i.leader_progress);
    let mut g = i.speed_glue[glue_index(c, p, i.leader_progress)];
    if d.abs() > c.far {
        g = if g >= ONE {
            q(g, i.g_speed)
        } else {
            q(g, recip(i.g_speed))
        };
    }
    let sp = i64::from(i.direction) * i64::from(i.leader_speed);
    // The curve is neutralised when the leader is stopped or held, when a held
    // leader is ahead of a car the curve would speed up, and when the car is
    // ahead of a slow leader.
    let held = i.leader_hold != 0;
    let leader_slow = sp < 131_072;
    if (leader_slow && held)
        || (p < i.leader_progress && held && g > ONE)
        || (leader_slow && p > i.leader_progress)
    {
        g = ONE;
    }
    fade_in(&i.fade, p, g)
}

/// Braking limit (Q16 m/s^2) of a scripted car.
pub fn brake_limit(is_racer: bool, non_sim_style: bool, pace: i32) -> i32 {
    let mut b = if is_racer { 15 * ONE } else { 12 * ONE };
    if non_sim_style {
        b *= 2;
    }
    if pace > ONE {
        b = q(b, pace);
    }
    b
}

/// Inputs of [`accel_limit`].
#[derive(Clone, Copy, Debug)]
pub struct AccelInput<'a> {
    pub speed: i32,
    pub shift_timer: i32,
    /// Car cap in Q16 m/s, present when the car type has one.
    pub car_cap: Option<i32>,
    pub tier_skill_factor: i32,
    pub pace: i32,
    /// Acceleration by whole m/s of speed, in 1/256 m/s^2.
    pub accel_table: &'a [i16],
    pub is_racer: bool,
    pub catchup_option: bool,
    pub leader_hold: i32,
    pub progress: i32,
    pub leader_progress: i32,
    pub direction: i32,
    pub leader_speed: i32,
    pub accel_glue: &'a [i32; GLUE_LEN],
    pub single_race_factor: Option<i32>,
    pub beginner: bool,
    pub aggressive: bool,
}

/// Maximum forward acceleration (Q16 m/s^2).
pub fn accel_limit(c: &RuleConstants, i: &AccelInput) -> i32 {
    if i.shift_timer > 0 {
        return 0;
    }
    if let Some(cap) = i.car_cap {
        if q(cap, i.tier_skill_factor) < i.speed || q(cap, i.pace) < i.speed || cap < i.speed {
            return 0;
        }
    }
    let index = (i.speed >> 16).unsigned_abs().min(111) as usize;
    let table = i.accel_table.get(index).copied().unwrap_or(0);
    let mut a = i32::from(table) * 256;
    if i.is_racer && i.catchup_option && i.leader_hold == 0 {
        let mut g = i.accel_glue[glue_index(c, i.progress, i.leader_progress)];
        if let Some(f) = i.single_race_factor {
            g = q(g, f);
        }
        if i64::from(i.direction) * i64::from(i.leader_speed) < 131_072 {
            g = ONE;
        }
        a = q(a, g);
    }
    if i.beginner && i.progress > c.late_after {
        a = q(a, 49_152);
    }
    if i.aggressive {
        a = q(a, 85_196);
    }
    a
}

/// Per-track pace trim: `(after_rows, sim, alt)`.
#[derive(Clone, Copy, Debug)]
pub struct Trim {
    pub after_rows: i32,
    pub sim: i32,
    pub alt: i32,
}

/// Inputs of [`target_speed`].
#[derive(Clone, Copy, Debug)]
pub struct TargetInput<'a> {
    pub direction: i32,
    pub rows: i32,
    pub row: i32,
    pub progress: i32,
    pub car_type: i32,
    /// Section speed in whole m/s for this row (already looked up).
    pub section_speed: i32,
    pub car_cap: Option<i32>,
    pub heading_error: i32,
    pub curve_hi: i32,
    pub curve_lo: i32,
    pub slope: i32,
    pub handling: i32,
    pub speed_glue: &'a [i32; GLUE_LEN],
    pub g_speed: i32,
    pub fade: [i32; 4],
    pub leader_progress: i32,
    pub leader_speed: i32,
    pub leader_hold: i32,
    pub use_tier: bool,
    pub tier_factor: i32,
    pub late_factor: i32,
    pub trim: Option<Trim>,
    pub sim_style: bool,
    pub human_rows: &'a [i32],
    pub finished: bool,
    /// Lateral offset and half width of the car (Q16 metres).
    pub lateral: i32,
    pub half_width: i32,
    pub lanes_left: i32,
    pub lanes_right: i32,
    /// Lane widths in half-metre units (a lane is `w / 2` metres).
    pub width_left: i32,
    pub width_right: i32,
}

/// Result of [`target_speed`]; `pace` is `None` in the finished state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub target: i32,
    pub pace: Option<i32>,
}

/// Compose the target speed (Q16 m/s, signed by direction) and the pace factor.
pub fn target_speed(c: &RuleConstants, i: &TargetInput) -> Target {
    let d = i64::from(i.direction);
    if i.finished {
        let left = -i64::from(i.lanes_left) * i64::from(i.width_left) * 32_768;
        let right = i64::from(i.lanes_right) * i64::from(i.width_right) * 32_768;
        let lateral = i64::from(i.lateral);
        let half = i64::from(i.half_width);
        let inside = lateral - half >= left && lateral + half <= right;
        let near_line = if i.direction >= 0 {
            i.row <= 60
        } else {
            i.row >= i.rows - 60
        };
        let target = if inside && near_line { d * 720_896 } else { d };
        return Target {
            target: wrap32(target),
            pace: None,
        };
    }
    let mut base = wrap32(i64::from(i.section_speed) * i64::from(ONE));
    if i.car_type < 15 {
        if let Some(cap) = i.car_cap {
            if base > cap {
                base = cap;
            }
        }
    }
    let curve = misalignment(i.heading_error, i.curve_hi, i.curve_lo, i.slope, i.handling);
    let glue = speed_catchup(
        c,
        &CatchUpInput {
            progress: i.progress,
            leader_progress: i.leader_progress,
            direction: i.direction,
            leader_speed: i.leader_speed,
            leader_hold: i.leader_hold,
            speed_glue: i.speed_glue,
            g_speed: i.g_speed,
            fade: i.fade,
        },
    );
    let skill = skill_factor(c, i.use_tier, i.tier_factor, i.late_factor, i.progress);
    let trim = match i.trim {
        Some(t) if i.progress > t.after_rows => {
            if i.sim_style {
                t.sim
            } else {
                t.alt
            }
        }
        _ => ONE,
    };
    let mut f = q(q(q(curve, glue), skill), trim);
    f = proximity_clamp(c, f, i.rows, i.row, i.human_rows);
    if i.progress < c.boost_window {
        f = q(f, ONE + (c.boost_window - i.progress) * c.boost_per_segment);
    }
    Target {
        target: wrap32(d * i64::from(q(base, f))),
        pace: Some(f),
    }
}
