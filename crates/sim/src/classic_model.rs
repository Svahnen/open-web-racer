//! Classic arcade-racer handling: the project's tuned feel model.
//!
//! The model is a set of tuned response curves, not a physical simulation. It
//! aims at the handling of a classic arcade racer: a car that launches with a
//! short clutch delay, pulls hard through a five or six speed gearbox, holds
//! its gear at the rev limit without cycling, brakes very hard, and steers with
//! a yaw rate that is linear in the steering command, flat at low speed, drops
//! at a fixed knee speed and then falls smoothly. The body leads the direction
//! of travel by exactly one tuning tick of yaw (the chase camera lags the body
//! in the renderer).
//!
//! All numbers are project-owned feel targets (see `tests/feel/classic-targets.json`
//! and `docs/CLASSIC_FEEL.md`). Per-car character comes from [`ClassicConfig`]:
//! shift points, top speed, acceleration, braking and engine drag scales, shift
//! delay, reverse speed, steering gain, grip and the yaw boost speed.
//!
//! Structure (all rates are per second of simulation time, so the model is
//! independent of the physics step; the tuning tick of 1/32 s is converted by
//! time, `rate_per_tick * 32`):
//!
//! * Control commands are rate-limited ramps on byte scales: throttle and brake
//!   0..=248, steering -124..=124.
//! * A speed-driven automatic gearbox with hysteresis; each shift briefly cuts
//!   drive and drag. A manual gearbox holds the gear and caps its speed.
//! * Forward acceleration: engine blend `g*A(gear, v) - (1-g)*D(v)` where `A` is
//!   the full-throttle net acceleration (piecewise linear per gear) and `D` the
//!   coasting deceleration by speed, minus `brake*B(v)`.
//! * Yaw rate = (steering command one tick ago / 124) * full-lock curve of speed.
//! * Travel direction equals the body heading one tick late in normal steering;
//!   the handbrake with steering starts a slide in which the velocity direction
//!   lags the body by up to about 70 degrees and then recovers.
//! * Reverse: the selector ladder R, N, drive; reverse works at any speed and
//!   brakes a forward-rolling car through zero.
//!
//! Arithmetic is IEEE `+ - * /` and `sqrt` plus the `sin`/`cos` of the heading
//! that every sim model uses. Powers and exponentials are evaluated with the
//! series in this file, so results do not depend on the platform math library.

use std::f64::consts::TAU;

use crate::drivetrain::{
    apply_auto_reverse, update_selector, DriveState, DriveTelemetry, COMMAND_FULL,
    GEAR_CODE_NEUTRAL, GEAR_CODE_REVERSE, SELECTOR_DRIVE, SELECTOR_NEUTRAL, SELECTOR_REVERSE,
    SLIDE_OFFSET_MAX, STEER_FULL,
};
use crate::provisional_model::KinematicUpdate;
use crate::{DriverInput, Vec2, Vehicle};

/// Revision of the classic equations; bound into world identity. Bump whenever
/// an equation or a table changes.
pub const CLASSIC_MODEL_REVISION: &str = "classic-feel-v2";
/// Largest supported forward gear count.
pub const CLASSIC_MAX_GEARS: usize = 8;

/// Tuning tick rate. Per-tick rates below are converted by time.
const TICKS_PER_SECOND: f64 = 32.0;

const THROTTLE_RISE_PER_S: f64 = 16.0 * TICKS_PER_SECOND;
const THROTTLE_FALL_PER_S: f64 = 47.7 * TICKS_PER_SECOND;
/// Brake command reaches full within 4 ticks.
const BRAKE_RATE_PER_S: f64 = 64.0 * TICKS_PER_SECOND;
const STEER_RISE_PER_S: f64 = 16.0 * TICKS_PER_SECOND;
const STEER_RELEASE_PER_S: f64 = 32.0 * TICKS_PER_SECOND;

/// Gear acceleration profiles of the four non-top gears of the reference five
/// speed layout: net full-throttle acceleration (m/s^2) at the low end, middle
/// and high end of the gear's speed range. Cars with another gear count
/// interpolate between these by gear position.
const GEAR_PROFILES: [(f64, f64, f64); 4] = [
    (15.43, 18.87, 24.26),
    (8.34, 9.62, 9.38),
    (9.29, 9.26, 8.01),
    (5.90, 5.78, 5.85),
];
/// Top-gear acceleration shape: position in the range from the last shift speed
/// to the top speed (0..=1) against acceleration (m/s^2).
const TOP_GEAR_SHAPE: [(f64, f64); 4] = [(0.0, 6.74), (0.3043, 6.71), (0.6607, 6.18), (1.0, 0.0)];
/// Coast deceleration knots (speed m/s, m/s^2).
const COAST: [(f64, f64); 6] = [
    (0.0, 0.0),
    (5.0, 1.81),
    (13.4, 5.00),
    (26.8, 10.42),
    (44.7, 9.92),
    (70.0, 9.92),
];
/// Brake deceleration knots (speed m/s, m/s^2).
const BRAKE: [(f64, f64); 6] = [
    (0.0, 22.0),
    (12.0, 22.0),
    (20.0, 48.98),
    (30.0, 59.60),
    (40.0, 59.67),
    (52.0, 59.72),
];
/// At standstill the car does not move until the throttle command passes this
/// value (a launch delay of 9 to 10 ticks).
const LAUNCH_THRESHOLD: f64 = 99.5;
/// Drive and drag (m/s^2 at full / zero throttle) while a shift is under way.
const SHIFT_DRIVE: f64 = 0.633;
const SHIFT_DRAG: f64 = 3.065;
/// A manual shift pauses the drive slightly longer than the automatic box.
const MANUAL_SHIFT_FACTOR: f64 = 1.0776;

// Steering law. The full-lock yaw rate `W(v)` has two regimes with a sharp knee:
// below the knee it is the car's flat low-speed rate; above it falls smoothly
// with speed along a shared shape scaled by the car's grip and steering gain.

/// Speed (m/s) of the knee, the same in every car.
pub const STEER_KNEE_MPS: f64 = 20.5;
/// Low-speed full-lock yaw rate per unit of steering gain (a starting point for
/// a car's `low_speed_yaw_rate`).
pub const STEER_LOW_PER_GAIN: f64 = 0.0951;
const STEER_GRIP_EXPONENT: f64 = 0.733;
const STEER_GAIN_EXPONENT: f64 = 0.877;
const STEER_REF_GRIP: f64 = 3.0;
const STEER_REF_GAIN: f64 = 18.0;
/// Shape `S(v)` (rad/s) of a car with grip 3.0 and steering gain 18.0.
const STEER_SHAPE: [(f64, f64); 15] = [
    (STEER_KNEE_MPS, 1.088),
    (22.0, 1.063),
    (25.0, 1.013),
    (30.0, 0.926),
    (35.0, 0.864),
    (40.0, 0.810),
    (45.0, 0.763),
    (50.0, 0.719),
    (55.0, 0.675),
    (60.0, 0.640),
    (65.0, 0.610),
    (70.0, 0.585),
    (80.0, 0.540),
    (100.0, 0.470),
    (140.0, 0.350),
];
/// From a car's `yaw_boost_speed_mps` the full-lock yaw rate is this much higher.
const STEER_BOOST_GAIN: f64 = 1.18;
/// Yaw authority fades linearly to zero below this speed so a stopped car
/// cannot rotate in place.
const YAW_FULL_AUTHORITY_MPS: f64 = 8.0;
/// The travel direction lags the body heading by this much time in normal
/// steering (one tuning tick).
const TRAVEL_LAG_SECONDS: f64 = 1.0 / TICKS_PER_SECOND;

// Handbrake.
/// Handbrake without steering: deceleration for the first 10 ticks, then the
/// steady rate (engine drive is cut while the handbrake is held).
const HANDBRAKE_INITIAL_DECEL: f64 = 15.0;
const HANDBRAKE_STEADY_DECEL: f64 = 3.6;
const HANDBRAKE_INITIAL_SECONDS: f64 = 10.0 / TICKS_PER_SECOND;
/// Handbrake with steering: deceleration `D0 + D1 * sin(offset)`.
const SLIDE_DECEL_BASE: f64 = 13.95;
const SLIDE_DECEL_PER_SIN_OFFSET: f64 = 16.0;
/// Slide yaw rate at full lock by speed (m/s), rad/s; multiplied by
/// `SLIDE_YAW_OFFSET_GAIN(offset)` and by `c * (2 - c)` of the steering
/// command fraction `c`.
const SLIDE_YAW_KNOTS: [(f64, f64); 8] = [
    (0.0, 0.8),
    (6.0, 0.8),
    (9.0, 1.75),
    (11.7, 2.35),
    (13.0, 3.1),
    (15.0, 2.7),
    (21.0, 2.45),
    (26.5, 2.3),
];
/// Yaw-rate gain by body-minus-travel offset magnitude (rad).
const SLIDE_YAW_OFFSET_GAIN: [(f64, f64); 6] = [
    (0.0, 1.0),
    (0.35, 1.0),
    (0.45, 1.34),
    (0.6, 1.58),
    (0.68, 0.97),
    (2.0, 1.0),
];
/// Velocity direction turn rate toward the body: `min(A / v, K * v)` rad/s,
/// faded in over the first 0.1 rad of offset.
const SLIDE_TURN_LATERAL_ACCEL: f64 = 29.1;
const SLIDE_TURN_PER_SPEED: [(f64, f64); 4] =
    [(0.0, 0.06), (10.0, 0.06), (16.0, 0.10), (26.0, 0.123)];
const SLIDE_TURN_FADE_RAD: f64 = 0.1;
/// Offset left by a slide decays by this factor per tuning tick.
const SLIDE_RECOVERY_PER_TICK: f64 = 0.90;
/// Substeps for the speed integration, to keep results step-size independent.
const SUBSTEPS: usize = 4;

// Reverse.
/// Speed (m/s) below which the car counts as standing for the reverse path.
const REVERSE_BACKWARD_EPS_MPS: f64 = 0.5;
/// Reverse selected while rolling forward is accepted at any speed: the car is
/// braked against its motion by `REVERSE_BRAKING_BASE` m/s^2 without throttle
/// and `+ REVERSE_BRAKING_PER_THROTTLE` with it, through zero.
const REVERSE_BRAKING_BASE: f64 = 20.4;
/// The braking builds up over the first quarter second after R is selected.
const REVERSE_ENGAGE_SECONDS: f64 = 0.25;
const REVERSE_ENGAGE_START: f64 = 0.45;
const REVERSE_BRAKING_PER_THROTTLE: f64 = 4.8;
/// A forward gear pulling against backward motion: thrust against the motion,
/// m/s^2 at full throttle.
const DRIVE_AGAINST_BACKWARD: f64 = 16.0;
/// Neutral coasting drag (m/s^2) is aerodynamic only, `k * v^2`.
const NEUTRAL_DRAG_PER_MPS2: f64 = 0.0011;
/// Reference reverse top speed of the tables below.
const REVERSE_REFERENCE_MPS: f64 = 29.4;
/// Reverse net acceleration at full throttle (speed m/s, m/s^2) for the
/// reference reverse speed; the speed axis is stretched for other cars.
const REVERSE_ACCEL: [(f64, f64); 7] = [
    (0.0, 16.8),
    (6.0, 18.0),
    (15.0, 17.5),
    (22.0, 17.0),
    (26.0, 10.0),
    (28.4, 3.5),
    (29.4, 0.0),
];
/// Reverse throttle-off deceleration.
const REVERSE_COAST: [(f64, f64); 4] = [(0.0, 0.0), (10.0, 6.5), (20.0, 9.5), (30.0, 10.5)];
/// Reverse full-brake deceleration.
const REVERSE_BRAKE: [(f64, f64); 3] = [(0.0, 20.0), (22.0, 23.0), (30.0, 26.0)];

// Manual gearbox.
/// Fraction of the speed cap of a gear below which the automatic box shifts up.
const AUTO_SHIFT_FRACTION_OF_CAP: f64 = 0.95;
/// Engine speed at the cap, as a fraction of the redline, and the idle level.
const RPM_AT_CAP: f64 = 0.984;
const RPM_IDLE: f64 = 0.18;
/// The speed approaches the cap within this fraction of it (no bounce).
const CAP_APPROACH_FRACTION: f64 = 0.015;
/// Engine braking (m/s^2) of a gear that is too low for the road speed.
const OVERREV_BRAKE_DECEL: f64 = 2.0;
/// A gear above the road speed it was built for pulls weakly at low engine
/// speed: the acceleration is scaled from `LOW_RPM_FLOOR` up to full as the
/// engine speed rises from `LOW_RPM_START` (fraction of the cap) to the lowest
/// speed of the gear's range.
const LOW_RPM_START: f64 = 0.15;
const LOW_RPM_FLOOR: f64 = 0.30;
/// The top gear's range ends this multiple of its lowest speed (HUD estimate).
const TOP_GEAR_RANGE_FACTOR: f64 = 1.16;

/// Parameters of one car for the classic model. SI units, m/s for speeds.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassicConfig {
    /// Speeds at which the automatic box shifts up from gear 1, 2, ...
    /// (ascending; the car has one more forward gear than entries).
    pub upshift_mps: Vec<f64>,
    /// Speeds below which the automatic box shifts down from gear 2, 3, ...
    /// (ascending, each below the matching `upshift_mps`).
    pub downshift_mps: Vec<f64>,
    /// Speed at which the top gear stops pulling.
    pub top_speed_mps: f64,
    /// Scale of the full-throttle acceleration of every gear.
    pub accel_scale: f64,
    /// Scale of the coasting (engine drag) deceleration.
    pub coast_scale: f64,
    /// Scale of the brake deceleration.
    pub brake_scale: f64,
    /// Drive pause of an automatic shift, seconds.
    pub shift_seconds: f64,
    /// Reverse top speed.
    pub reverse_speed_mps: f64,
    /// Steering gain: sets the above-knee yaw level (about 18 to 20 for a
    /// typical car).
    pub steer_gain: f64,
    /// Grip: sets the above-knee yaw level (about 2.8 to 3.6).
    pub grip: f64,
    /// Full-lock yaw rate below the knee, rad/s (about `STEER_LOW_PER_GAIN *
    /// steer_gain`).
    pub low_speed_yaw_rate: f64,
    /// Speed from which the full-lock yaw rate is boosted.
    pub yaw_boost_speed_mps: f64,
}

impl Default for ClassicConfig {
    /// The reference car: five gears, quick launch, 80 m/s top speed.
    fn default() -> Self {
        Self {
            upshift_mps: vec![27.448_701, 39.921_320, 52.304_529, 62.318_387],
            downshift_mps: vec![22.754_705, 33.126_201, 44.525_906, 53.198_623],
            top_speed_mps: 80.0,
            accel_scale: 1.0,
            coast_scale: 1.0,
            brake_scale: 1.0,
            shift_seconds: 0.2755,
            reverse_speed_mps: REVERSE_REFERENCE_MPS,
            steer_gain: 18.3,
            grip: 3.3,
            low_speed_yaw_rate: 1.7313,
            yaw_boost_speed_mps: 68.64,
        }
    }
}

impl ClassicConfig {
    /// Check ranges; the model assumes a validated configuration.
    pub fn validate(&self) -> Result<(), &'static str> {
        let n = self.upshift_mps.len();
        if n == 0 || n >= CLASSIC_MAX_GEARS || self.downshift_mps.len() != n {
            return Err("classic gear count must be 2..=8 with matching shift lists");
        }
        let all = self
            .upshift_mps
            .iter()
            .chain(self.downshift_mps.iter())
            .chain([
                &self.top_speed_mps,
                &self.accel_scale,
                &self.coast_scale,
                &self.brake_scale,
                &self.shift_seconds,
                &self.reverse_speed_mps,
                &self.steer_gain,
                &self.grip,
                &self.low_speed_yaw_rate,
                &self.yaw_boost_speed_mps,
            ]);
        if all.into_iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("classic parameters must be finite and positive");
        }
        let ascending = |list: &[f64]| list.windows(2).all(|w| w[1] > w[0]);
        if !ascending(&self.upshift_mps) || !ascending(&self.downshift_mps) {
            return Err("classic shift speeds must ascend");
        }
        if self
            .upshift_mps
            .iter()
            .zip(&self.downshift_mps)
            .any(|(up, down)| down >= up)
            || self.upshift_mps[0] < 5.0
        {
            return Err("classic downshift speeds must lie below their upshift speeds");
        }
        let last = self.upshift_mps[n - 1];
        if self.top_speed_mps < last * 1.05 || self.top_speed_mps > 200.0 {
            return Err(
                "classic top speed must exceed the last shift speed and stay below 200 m/s",
            );
        }
        if !(0.3..=3.0).contains(&self.accel_scale)
            || !(0.3..=3.0).contains(&self.coast_scale)
            || !(0.3..=3.0).contains(&self.brake_scale)
            || !(0.02..=0.6).contains(&self.shift_seconds)
            || !(5.0..=60.0).contains(&self.reverse_speed_mps)
            || !(2.0..=60.0).contains(&self.steer_gain)
            || !(0.5..=10.0).contains(&self.grip)
            || !(0.2..=4.0).contains(&self.low_speed_yaw_rate)
            || !(STEER_KNEE_MPS..=self.top_speed_mps).contains(&self.yaw_boost_speed_mps)
        {
            return Err("classic parameter outside its supported range");
        }
        Ok(())
    }

    /// Bit patterns of every parameter, for world identity hashing.
    pub fn identity_words(&self) -> Vec<u64> {
        let mut words: Vec<u64> = vec![self.upshift_mps.len() as u64];
        words.extend(self.upshift_mps.iter().map(|v| v.to_bits()));
        words.extend(self.downshift_mps.iter().map(|v| v.to_bits()));
        words.extend(
            [
                self.top_speed_mps,
                self.accel_scale,
                self.coast_scale,
                self.brake_scale,
                self.shift_seconds,
                self.reverse_speed_mps,
                self.steer_gain,
                self.grip,
                self.low_speed_yaw_rate,
                self.yaw_boost_speed_mps,
            ]
            .iter()
            .map(|v| v.to_bits()),
        );
        words
    }

    /// Highest forward gear number.
    pub fn gear_count(&self) -> u8 {
        (self.upshift_mps.len() + 1).min(CLASSIC_MAX_GEARS) as u8
    }

    /// Lowest and highest speed of a gear's range (the automatic box's shift
    /// points; the top gear ends a fixed multiple above its start).
    fn gear_speed_range(&self, gear: u8) -> (f64, f64) {
        let index = usize::from(gear.clamp(1, self.gear_count())) - 1;
        let last = self.upshift_mps.len();
        let low = if index == 0 {
            0.0
        } else {
            self.upshift_mps[index - 1]
        };
        let high = if index < last {
            self.upshift_mps[index]
        } else {
            self.upshift_mps[last - 1] * TOP_GEAR_RANGE_FACTOR
        };
        (low, high)
    }

    /// Speed cap (m/s) of a forward gear: the road speed at which the engine
    /// reaches the rev limit and the speed locks. The automatic shift points sit
    /// at 0.95 of it; the top gear's cap is the car's top speed.
    pub fn gear_cap_mps(&self, gear: u8) -> f64 {
        let index = usize::from(gear.clamp(1, self.gear_count())) - 1;
        if index == self.upshift_mps.len() {
            self.top_speed_mps
        } else {
            self.upshift_mps[index] / AUTO_SHIFT_FRACTION_OF_CAP
        }
    }

    /// Full-throttle net acceleration (m/s^2) of a gear at a speed.
    fn drive_acceleration(&self, gear: u8, speed: f64) -> f64 {
        let gear = gear.clamp(1, self.gear_count());
        let index = usize::from(gear) - 1;
        let non_top = self.upshift_mps.len();
        if index == non_top {
            let (low, _) = self.gear_speed_range(gear);
            let u = (speed - low) / (self.top_speed_mps - low);
            return self.accel_scale * interpolate(&TOP_GEAR_SHAPE, u);
        }
        let (low_speed, high_speed) = self.gear_speed_range(gear);
        let (low, mid, high) = gear_profile(index, non_top);
        let t = ((speed - low_speed) / (high_speed - low_speed)).clamp(-0.5, 1.5);
        let value = if t <= 0.5 {
            low + (mid - low) * (t / 0.5)
        } else {
            mid + (high - mid) * ((t - 0.5) / 0.5)
        };
        self.accel_scale * value
    }

    /// Scale of the drive acceleration of a (manual) gear below its own speed
    /// range: a high gear at low road speed pulls weakly.
    fn low_rpm_factor(&self, gear: u8, speed: f64) -> f64 {
        let (low_speed, _) = self.gear_speed_range(gear);
        if low_speed <= 0.0 || speed >= low_speed {
            return 1.0;
        }
        let cap = self.gear_cap_mps(gear);
        let along =
            ((speed / cap - LOW_RPM_START) / (low_speed / cap - LOW_RPM_START)).clamp(0.0, 1.0);
        LOW_RPM_FLOOR + (1.0 - LOW_RPM_FLOOR) * along
    }

    fn coast_deceleration(&self, speed: f64) -> f64 {
        self.coast_scale * interpolate(&COAST, speed)
    }

    fn brake_deceleration(&self, speed: f64) -> f64 {
        self.brake_scale * interpolate(&BRAKE, speed)
    }

    /// Brake deceleration at low speed, m/s^2 (for drivers' limits).
    pub fn low_speed_brake_mps2(&self) -> f64 {
        self.brake_deceleration(0.0)
    }

    /// Full-lock yaw rate `W(v)` (rad/s, magnitude) at a speed (m/s).
    pub fn full_lock_yaw_rate(&self, speed: f64) -> f64 {
        let speed = speed.abs();
        let authority = (speed / YAW_FULL_AUTHORITY_MPS).clamp(0.0, 1.0);
        let rate = if speed <= STEER_KNEE_MPS {
            self.low_speed_yaw_rate
        } else {
            let step = if speed >= self.yaw_boost_speed_mps {
                STEER_BOOST_GAIN
            } else {
                1.0
            };
            let level = pow(self.grip / STEER_REF_GRIP, STEER_GRIP_EXPONENT)
                * pow(self.steer_gain / STEER_REF_GAIN, STEER_GAIN_EXPONENT);
            interpolate(&STEER_SHAPE, speed) * level * step
        };
        rate * authority
    }

    /// Yaw rate (rad/s, sim sign) for the (one tick delayed) steering command
    /// -124..124: linear in the command.
    pub fn yaw_rate(&self, command: f64, speed: f64) -> f64 {
        (command / STEER_FULL).clamp(-1.0, 1.0) * self.full_lock_yaw_rate(speed)
    }

    /// Engine speed estimate for audio/HUD of the automatic box: a sawtooth
    /// across each gear's speed range with a throttle lift.
    pub fn rpm01(&self, speed: f64, gear: u8, throttle01: f64) -> f64 {
        let gear = gear.clamp(1, self.gear_count());
        let (low, high) = self.gear_speed_range(gear);
        let span = (high - low).max(1.0e-6);
        let along = ((speed - low) / span).clamp(0.0, 1.0);
        let base = if gear == 1 {
            0.18 + 0.72 * (speed / high).clamp(0.0, 1.0)
        } else {
            0.45 + 0.50 * along
        };
        (base + 0.05 * throttle01.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Engine speed of a manual box in a gear: proportional to the road speed,
    /// at 0.984 of the redline at the gear's cap and pushed to the redline by a
    /// gear that is too low; clutch slip lifts it at low speed with the throttle.
    pub fn manual_rpm01(&self, speed: f64, gear: u8, throttle01: f64) -> f64 {
        let cap = self.gear_cap_mps(gear);
        let ratio = (speed / cap).max(0.0);
        let base = RPM_IDLE + (RPM_AT_CAP - RPM_IDLE) * ratio;
        let slip = 0.25 * throttle01.clamp(0.0, 1.0) * (1.0 - ratio).max(0.0);
        (base + slip).clamp(0.0, 1.0)
    }
}

/// Acceleration profile of non-top gear `index` (0-based) of a car with
/// `non_top` such gears, interpolated from the reference profiles by position.
fn gear_profile(index: usize, non_top: usize) -> (f64, f64, f64) {
    let last = GEAR_PROFILES.len() - 1;
    let position = if non_top <= 1 {
        0.0
    } else {
        index as f64 * last as f64 / (non_top - 1) as f64
    };
    let lower = (position.floor() as usize).min(last);
    let upper = (lower + 1).min(last);
    let f = position - lower as f64;
    let (a, b) = (GEAR_PROFILES[lower], GEAR_PROFILES[upper]);
    (
        a.0 + (b.0 - a.0) * f,
        a.1 + (b.1 - a.1) * f,
        a.2 + (b.2 - a.2) * f,
    )
}

fn interpolate(knots: &[(f64, f64)], x: f64) -> f64 {
    if x <= knots[0].0 {
        return knots[0].1;
    }
    for pair in knots.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    knots[knots.len() - 1].1
}

fn approach(current: f64, target: f64, rise: f64, fall: f64) -> f64 {
    if target > current {
        target.min(current + rise)
    } else {
        target.max(current - fall)
    }
}

const LN_2: f64 = std::f64::consts::LN_2;

/// `e^x` from `+ - * /` only (range reduction by powers of two, then a Taylor
/// series), so the value is the same on every platform.
fn exp(x: f64) -> f64 {
    let x = x.clamp(-700.0, 700.0);
    let k = (x / LN_2).round();
    let r = x - k * LN_2;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=16 {
        term *= r / f64::from(n);
        sum += term;
    }
    let mut scale = 1.0;
    let mut remaining = k as i32;
    while remaining > 0 {
        scale *= 2.0;
        remaining -= 1;
    }
    while remaining < 0 {
        scale *= 0.5;
        remaining += 1;
    }
    sum * scale
}

/// Natural logarithm of a positive number from `+ - * /` only.
fn ln(x: f64) -> f64 {
    let mut x = x.max(f64::MIN_POSITIVE);
    let mut k = 0.0;
    while x > 1.5 {
        x *= 0.5;
        k += 1.0;
    }
    while x < 0.75 {
        x *= 2.0;
        k -= 1.0;
    }
    let s = (x - 1.0) / (x + 1.0);
    let s2 = s * s;
    let mut power = s;
    let mut sum = 0.0;
    for n in 0..14 {
        sum += power / f64::from(2 * n + 1);
        power *= s2;
    }
    2.0 * sum + k * LN_2
}

/// `base^exponent` for a positive base, platform independent.
fn pow(base: f64, exponent: f64) -> f64 {
    exp(exponent * ln(base))
}

/// HUD/audio view of a classic drivetrain state.
pub fn telemetry(cfg: &ClassicConfig, drive: &DriveState, speed: f64) -> DriveTelemetry {
    let throttle01 = drive.throttle_command / COMMAND_FULL;
    match drive.selector {
        SELECTOR_REVERSE => DriveTelemetry {
            gear: GEAR_CODE_REVERSE,
            rpm01: cfg.rpm01(speed, 1, throttle01),
        },
        SELECTOR_NEUTRAL => DriveTelemetry {
            gear: GEAR_CODE_NEUTRAL,
            // Throttle in neutral takes the engine to the full redline.
            rpm01: (RPM_IDLE + (1.0 - RPM_IDLE) * throttle01).clamp(0.0, 1.0),
        },
        _ if drive.manual => DriveTelemetry {
            gear: drive.gear,
            rpm01: cfg.manual_rpm01(speed, drive.gear, throttle01),
        },
        _ => DriveTelemetry {
            gear: drive.gear,
            rpm01: cfg.rpm01(speed, drive.gear, throttle01),
        },
    }
}

fn slide_yaw_rate(command: f64, speed: f64, offset: f64) -> f64 {
    let fraction = (command / STEER_FULL).clamp(-1.0, 1.0);
    let magnitude = fraction.abs();
    interpolate(&SLIDE_YAW_KNOTS, speed)
        * interpolate(&SLIDE_YAW_OFFSET_GAIN, offset.abs())
        * magnitude
        * (2.0 - magnitude)
        * fraction.signum()
}

/// Handbrake deceleration (m/s^2): the no-steer profile blended into the slide
/// profile as steering comes on.
fn handbrake_deceleration(held_seconds: f64, steer_fraction: f64, offset: f64) -> f64 {
    let plain = if held_seconds < HANDBRAKE_INITIAL_SECONDS {
        HANDBRAKE_INITIAL_DECEL
    } else {
        HANDBRAKE_STEADY_DECEL
    };
    let slide = SLIDE_DECEL_BASE + SLIDE_DECEL_PER_SIN_OFFSET * offset.abs().sin();
    let weight = (steer_fraction.abs() / 0.25).clamp(0.0, 1.0);
    plain + (slide - plain) * weight
}

/// Smooth the throttle, brake and steering commands one step; returns the
/// steering command before the step.
fn smooth_commands(drive: &mut DriveState, input: &DriverInput, dt: f64) -> f64 {
    let steer_before = drive.steer_command;
    drive.throttle_command = approach(
        drive.throttle_command,
        input.throttle * COMMAND_FULL,
        THROTTLE_RISE_PER_S * dt,
        THROTTLE_FALL_PER_S * dt,
    );
    drive.brake_command = approach(
        drive.brake_command,
        input.brake * COMMAND_FULL,
        BRAKE_RATE_PER_S * dt,
        BRAKE_RATE_PER_S * dt,
    );
    let steer_target = input.steer * STEER_FULL;
    drive.steer_command = if steer_target * drive.steer_command < 0.0 {
        // Reversing: release through zero first.
        approach(
            drive.steer_command,
            0.0,
            STEER_RELEASE_PER_S * dt,
            STEER_RELEASE_PER_S * dt,
        )
    } else if steer_target.abs() > drive.steer_command.abs() {
        approach(
            drive.steer_command,
            steer_target,
            STEER_RISE_PER_S * dt,
            STEER_RISE_PER_S * dt,
        )
    } else {
        approach(
            drive.steer_command,
            steer_target,
            STEER_RELEASE_PER_S * dt,
            STEER_RELEASE_PER_S * dt,
        )
    };
    steer_before
}

/// Reverse driving and backward rolling. The body axis is straight; `speed` is
/// the speed backward (negative while the car still rolls forward after reverse
/// was selected: the reverse gear then brakes it hard through zero). Neutral and
/// drive only appear here while the car rolls backward.
fn advance_reverse(
    cfg: &ClassicConfig,
    vehicle: Vehicle,
    input: DriverInput,
    dt: f64,
    drive: &mut DriveState,
) -> KinematicUpdate {
    let direction = Vec2::new(vehicle.heading.cos(), vehicle.heading.sin());
    let mut speed = -vehicle.velocity.dot(direction);
    smooth_commands(drive, &input, dt);
    let throttle = drive.throttle_command / COMMAND_FULL;
    let brake = drive.brake_command / COMMAND_FULL;
    let reverse_stretch = cfg.reverse_speed_mps / REVERSE_REFERENCE_MPS;
    let sub = dt / SUBSTEPS as f64;
    for _ in 0..SUBSTEPS {
        if speed < 0.0 {
            // Rolling forward in reverse: braked against the motion.
            let engage = 1.0 - (drive.shift_timer / REVERSE_ENGAGE_SECONDS).clamp(0.0, 1.0);
            let against = (REVERSE_BRAKING_BASE + REVERSE_BRAKING_PER_THROTTLE * throttle)
                * (REVERSE_ENGAGE_START + (1.0 - REVERSE_ENGAGE_START) * engage);
            let braking = brake * cfg.brake_deceleration(-speed);
            speed = (speed + against.max(braking) * sub).min(0.0);
            continue;
        }
        let engine = match drive.selector {
            SELECTOR_REVERSE => {
                throttle * interpolate(&REVERSE_ACCEL, speed / reverse_stretch)
                    - (1.0 - throttle) * interpolate(&REVERSE_COAST, speed)
            }
            SELECTOR_DRIVE => {
                -throttle * DRIVE_AGAINST_BACKWARD - interpolate(&REVERSE_COAST, speed)
            }
            _ => -NEUTRAL_DRAG_PER_MPS2 * speed * speed,
        };
        let deceleration = brake * interpolate(&REVERSE_BRAKE, speed);
        speed = (speed + ((1.0 - brake) * engine - deceleration) * sub).max(0.0);
    }
    drive.shift_timer = (drive.shift_timer - dt).max(0.0);
    drive.slide_offset = 0.0;
    drive.handbrake_seconds = if input.handbrake {
        drive.handbrake_seconds + dt
    } else {
        0.0
    };
    // Steering acts on the backward-moving body with reversed sense.
    let sense = if speed >= 0.0 { -1.0 } else { 1.0 };
    let yaw = sense * cfg.yaw_rate(drive.steer_command, speed.abs());
    let heading = (vehicle.heading + yaw * dt).rem_euclid(TAU);
    let velocity = Vec2::new(heading.cos(), heading.sin()) * -speed;
    KinematicUpdate {
        heading,
        angular_velocity: yaw,
        velocity,
        proposed_position: vehicle.position + velocity * dt,
    }
}

/// Advance the classic model by `dt` seconds of simulation time.
pub(crate) fn advance(
    cfg: &ClassicConfig,
    vehicle: Vehicle,
    input: DriverInput,
    dt: f64,
    drive: &mut DriveState,
    road_slope: f64,
) -> KinematicUpdate {
    let gear_count = cfg.gear_count();
    let travel_dir = vehicle.heading - drive.slide_offset;
    let selector_before = drive.selector;
    let forward_mps = vehicle
        .velocity
        .dot(Vec2::new(travel_dir.cos(), travel_dir.sin()));
    let input = apply_auto_reverse(drive, input, forward_mps);
    if update_selector(drive, &input, gear_count).forward_shift {
        drive.shift_timer = cfg.shift_seconds * MANUAL_SHIFT_FACTOR;
    }
    if selector_before != SELECTOR_REVERSE && drive.selector == SELECTOR_REVERSE {
        drive.shift_timer = REVERSE_ENGAGE_SECONDS;
    }
    // Reverse selected (at any speed), or rolling backward in another gear.
    if drive.selector == SELECTOR_REVERSE || forward_mps < -REVERSE_BACKWARD_EPS_MPS {
        return advance_reverse(cfg, vehicle, input, dt, drive);
    }
    // The velocity set by the previous step points along `heading - offset`.
    // Collisions can leave it deflected or backwards; only the component along
    // the travel direction carries on.
    let travel = vehicle.heading - drive.slide_offset;
    let direction = Vec2::new(travel.cos(), travel.sin());
    let mut speed = vehicle.velocity.dot(direction).max(0.0);
    let speed_at_start = speed;
    let offset_at_start = drive.slide_offset;

    let steer_before = smooth_commands(drive, &input, dt);

    // The body yaws in response to the command as it was one tuning tick ago.
    // The command ramps linearly, so the delayed value is the previous value
    // extrapolated back along this step's slope; exact at 32 Hz, bounded by the
    // values in between at other rates.
    let yaw_command = {
        let slope = (drive.steer_command - steer_before) / dt;
        let delayed = steer_before - slope * (TRAVEL_LAG_SECONDS - dt).max(0.0);
        let bound = steer_before.abs().max(drive.steer_command.abs());
        let toward = if drive.steer_command != 0.0 {
            drive.steer_command.signum()
        } else {
            steer_before.signum()
        };
        (delayed * toward).clamp(0.0, bound) * toward
    };

    let throttle = drive.throttle_command / COMMAND_FULL;
    let brake = drive.brake_command / COMMAND_FULL;

    // Automatic gearbox with hysteresis between up and down thresholds.
    let previous_gear = drive.gear;
    let mut gear = drive.gear.clamp(1, gear_count);
    if !drive.manual {
        while gear < gear_count && speed > cfg.upshift_mps[usize::from(gear) - 1] {
            gear += 1;
        }
        while gear > 1 && speed < cfg.downshift_mps[usize::from(gear) - 2] {
            gear -= 1;
        }
        if gear != previous_gear && brake < 0.5 {
            drive.shift_timer = cfg.shift_seconds;
        }
    }
    drive.gear = gear;
    let cap = cfg.gear_cap_mps(gear);

    let handbrake = input.handbrake;
    drive.handbrake_seconds = if handbrake {
        drive.handbrake_seconds + dt
    } else {
        0.0
    };
    let engaged_throttle = if drive.selector != SELECTOR_DRIVE
        || (speed < 0.05 && drive.throttle_command < LAUNCH_THRESHOLD)
    {
        0.0
    } else {
        throttle
    };
    let sub = dt / SUBSTEPS as f64;
    for _ in 0..SUBSTEPS {
        if handbrake {
            // Engine drive is cut; the handbrake dominates the deceleration.
            let decel = handbrake_deceleration(
                drive.handbrake_seconds,
                yaw_command / STEER_FULL,
                offset_at_start,
            );
            speed = (speed - decel * sub).max(0.0);
            continue;
        }
        let engine = if drive.shift_timer > 0.0 {
            engaged_throttle * SHIFT_DRIVE - (1.0 - engaged_throttle) * SHIFT_DRAG
        } else if drive.selector != SELECTOR_DRIVE {
            // Neutral: no engine braking, aerodynamic drag only.
            -NEUTRAL_DRAG_PER_MPS2 * speed * speed
        } else if drive.manual {
            // The gear holds its speed at the cap (no limiter cycling); a gear
            // too low for the road speed over-revs and brakes the car.
            let taper = ((cap - speed) / (CAP_APPROACH_FRACTION * cap)).clamp(0.0, 1.0);
            let pull = engaged_throttle
                * cfg.drive_acceleration(gear, speed)
                * cfg.low_rpm_factor(gear, speed)
                * taper;
            let drag = (1.0 - engaged_throttle) * cfg.coast_deceleration(speed);
            let over = if speed > cap {
                OVERREV_BRAKE_DECEL
            } else {
                0.0
            };
            pull - drag - over
        } else {
            engaged_throttle * cfg.drive_acceleration(gear, speed)
                - (1.0 - engaged_throttle) * cfg.coast_deceleration(speed)
        };
        // The brake curve is the total deceleration at full brake: engine drag
        // is blended out as the brake comes on.
        let deceleration = brake * cfg.brake_deceleration(speed);
        speed = (speed + ((1.0 - brake) * engine - deceleration) * sub).max(0.0);
    }
    drive.shift_timer = (drive.shift_timer - dt).max(0.0);

    let tick_decay = exp(dt * TICKS_PER_SECOND * ln(SLIDE_RECOVERY_PER_TICK));
    let (angular_velocity, slide_offset) = if handbrake {
        // Slide: the body yaws at the slide rate while the velocity direction
        // turns toward it at a limited rate.
        let yaw = slide_yaw_rate(yaw_command, speed_at_start, offset_at_start);
        let turn = (SLIDE_TURN_LATERAL_ACCEL / speed_at_start.max(1.0))
            .min(interpolate(&SLIDE_TURN_PER_SPEED, speed_at_start) * speed_at_start)
            * (offset_at_start.abs() / SLIDE_TURN_FADE_RAD).min(1.0)
            * offset_at_start.signum();
        (yaw, offset_at_start + (yaw - turn) * dt)
    } else {
        // Normal steering: travel direction = body heading one tick late; any
        // slide offset left over decays by 0.90 per tick.
        let yaw = cfg.yaw_rate(yaw_command, speed)
            + crate::road_pull::slope_yaw_rate(
                road_slope,
                speed,
                throttle,
                cfg.full_lock_yaw_rate(speed),
            );
        let previous_lag = vehicle.angular_velocity * TRAVEL_LAG_SECONDS;
        let mut excess = (offset_at_start - previous_lag) * tick_decay;
        if excess.abs() < 1.0e-9 {
            excess = 0.0;
        }
        (yaw, yaw * TRAVEL_LAG_SECONDS + excess)
    };
    drive.slide_offset = slide_offset.clamp(-SLIDE_OFFSET_MAX, SLIDE_OFFSET_MAX);
    let heading = (vehicle.heading + angular_velocity * dt).rem_euclid(TAU);
    let travel = heading - drive.slide_offset;
    let velocity = Vec2::new(travel.cos(), travel.sin()) * speed;
    KinematicUpdate {
        heading,
        angular_velocity,
        velocity,
        proposed_position: vehicle.position + velocity * dt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_match_the_platform_math_closely() {
        for x in [-9.5f64, -1.0, -0.1, 0.0, 0.3, 2.0, 11.0] {
            assert!((exp(x) / x.exp() - 1.0).abs() < 1e-13, "exp {x}");
        }
        for x in [0.01f64, 0.4, 0.9, 1.0, 1.7, 3.3, 250.0] {
            assert!((ln(x) - x.ln()).abs() < 1e-13, "ln {x}");
        }
        assert!((pow(1.1, 0.733) / 1.1f64.powf(0.733) - 1.0).abs() < 1e-13);
    }

    #[test]
    fn default_config_is_valid_and_varied_configs_validate() {
        assert_eq!(ClassicConfig::default().validate(), Ok(()));
        let mut c = ClassicConfig::default();
        c.upshift_mps.pop();
        c.downshift_mps.pop();
        assert_eq!(c.gear_count(), 4);
        assert_eq!(c.validate(), Ok(()));
        c.downshift_mps[0] = c.upshift_mps[0];
        assert!(c.validate().is_err());
        let mut c = ClassicConfig::default();
        c.top_speed_mps = 60.0;
        assert!(c.validate().is_err());
        let mut c = ClassicConfig::default();
        c.grip = f64::NAN;
        assert!(c.validate().is_err());
    }

    #[test]
    fn gear_profiles_interpolate_for_other_gear_counts() {
        assert_eq!(gear_profile(0, 4), GEAR_PROFILES[0]);
        assert_eq!(gear_profile(3, 4), GEAR_PROFILES[3]);
        assert_eq!(gear_profile(0, 5), GEAR_PROFILES[0]);
        assert_eq!(gear_profile(4, 5), GEAR_PROFILES[3]);
        let mid = gear_profile(1, 5);
        assert!(mid.0 < GEAR_PROFILES[0].0 && mid.0 > GEAR_PROFILES[1].0);
        assert_eq!(gear_profile(0, 1), GEAR_PROFILES[0]);
    }
}
