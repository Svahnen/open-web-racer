//! Generic, project-owned arcade vehicle model.
//!
//! This is the model the public engine ships as its default. It is designed
//! from general vehicle-dynamics knowledge only (a torque-curve engine behind
//! an automatic gearbox, tyre grip limits, aerodynamic and rolling drag, a
//! speed-sensitive steering lock and a first-order lateral tyre response); it
//! has no relation to any commercial title and no data derived from one.
//!
//! The body frame has `d` forward and `l` to the left. World velocity is
//! carried by [`Vehicle::velocity`]; each step splits it into a forward part
//! `v_f` and a sideways part `v_s`. The tyres remove `v_s` at a finite rate
//! (`lateral_grip_per_s`) and the body yaws at a rate that follows the
//! steering, so while a turn is held the body points a little past the
//! direction of travel (the "lead") and settles once the steering is released.
//!
//! Dynamic state lives in [`DriveState`] (smoothed commands, gear, selector),
//! so rollback frames need no extra fields:
//! `throttle_command` and `brake_command` are 0..=248, `steer_command` is
//! -124..=124 (positive turns counter-clockwise), `slide_offset` is the
//! body-minus-travel angle and `shift_timer` the remaining shift pause.
//!
//! Arithmetic is IEEE `+ - * /` and `sqrt` plus the `sin`/`cos` of the heading
//! that every sim model uses; there is no `exp`, `pow` or lookup of
//! platform-dependent behaviour, and every relaxation is written in its
//! unconditionally stable rational form.

use std::f64::consts::TAU;

use crate::drivetrain::{
    apply_auto_reverse, update_selector, DriveState, DriveTelemetry, COMMAND_FULL,
    GEAR_CODE_NEUTRAL, GEAR_CODE_REVERSE, SELECTOR_DRIVE, SELECTOR_NEUTRAL, SELECTOR_REVERSE,
    STEER_FULL,
};
use crate::provisional_model::KinematicUpdate;
use crate::{DriverInput, Vec2, Vehicle};

/// Revision of the arcade equations; bound into world identity.
pub const ARCADE_MODEL_REVISION: &str = "arcade-generic-v3";
/// Largest supported forward gear count.
pub const ARCADE_MAX_GEARS: usize = 12;

const G: f64 = 9.81;
/// Air density, kg/m^3.
const RHO: f64 = 1.2;
/// Seconds of drive cut during a gear change by the automatic box, and the
/// slightly longer pause of a manual shift.
const SHIFT_SECONDS: f64 = 0.12;
const MANUAL_SHIFT_SECONDS: f64 = 0.18;
/// Throttle command rise and fall, full scale per second.
const THROTTLE_RISE: f64 = 6.0;
const THROTTLE_FALL: f64 = 10.0;
/// Brake command rise and fall, full scale per second.
const BRAKE_RATE: f64 = 12.0;
/// Time constant of the yaw rate following its target, seconds.
const YAW_TAU: f64 = 0.07;
/// Upper bound on the yaw rate, rad/s.
const MAX_YAW_RATE: f64 = 3.2;
/// Reverse top speed as a fraction of the forward top speed.
const REVERSE_SPEED_FRACTION: f64 = 0.25;
/// Deceleration (m/s^2) of the driven wheels against the motion when reverse
/// is selected while rolling forward; the tyres still limit it to their grip.
const REVERSE_BRAKE_DECEL: f64 = 18.0;
/// Deceleration (m/s^2) of engine braking while the engine is pushed past the
/// redline by the road speed (a downshift that is too low for the speed).
const OVERREV_BRAKE_DECEL: f64 = 3.0;
/// Fraction of the redline at which a gear holds its speed. The torque fades
/// out over the last 3 percent of the range, so the car settles at the limit
/// without bouncing.
const REV_HOLD_RPM_FRACTION: f64 = 0.985;
/// Share of the engine range an upshift waits for, and the share a lower gear
/// may not exceed after a downshift.
const UPSHIFT_RANGE: f64 = 0.88;
const DOWNSHIFT_RANGE: f64 = 0.80;
/// Clutch slip: engine speed used for torque below this range fraction.
const LAUNCH_RANGE: f64 = 0.30;
/// Handbrake: rear brake deceleration (in g), grip loss factors, yaw boost.
const HANDBRAKE_DECEL_G: f64 = 0.35;
const HANDBRAKE_AMAX: f64 = 0.55;
const HANDBRAKE_GRIP: f64 = 0.35;
const HANDBRAKE_YAW: f64 = 1.3;
/// Arcade cornering: sideways acceleration the tyres can hold, as a multiple
/// of the plain grip limit (`tyre_grip` x g plus downforce). Braking and
/// traction keep the plain limit; cornering is deliberately generous so that a
/// car at full lock still turns a 80 to 120 m radius at 50 m/s (2.5 to 3 g).
const CORNER_GRIP_GAIN: f64 = 2.5;
/// Largest value stored in `DriveState::slide_offset` (matches its range).
const LEAD_MAX: f64 = 1.6;

/// Parameters of one car for the arcade model. SI units.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcadeConfig {
    pub mass_kg: f64,
    pub wheelbase_m: f64,
    pub wheel_radius_m: f64,
    pub peak_torque_nm: f64,
    pub idle_rpm: f64,
    pub redline_rpm: f64,
    pub gear_ratios: Vec<f64>,
    pub reverse_ratio: f64,
    pub final_drive: f64,
    pub drivetrain_efficiency: f64,
    pub max_brake_force_n: f64,
    pub drag_area_m2: f64,
    pub downforce_area_m2: f64,
    pub rolling_resistance: f64,
    pub tyre_grip: f64,
    pub max_steer_angle_rad: f64,
    pub steer_falloff_speed_mps: f64,
    pub max_speed_mps: f64,
    pub front_drive_bias: f64,
    pub steer_rate_per_s: f64,
    pub steer_return_per_s: f64,
    pub lateral_grip_per_s: f64,
}

impl Default for ArcadeConfig {
    /// A generic rear-driven coupe.
    fn default() -> Self {
        Self {
            mass_kg: 1180.0,
            wheelbase_m: 2.5,
            wheel_radius_m: 0.31,
            peak_torque_nm: 340.0,
            idle_rpm: 900.0,
            redline_rpm: 7200.0,
            gear_ratios: vec![3.4, 2.2, 1.55, 1.15, 0.9],
            reverse_ratio: 3.2,
            final_drive: 3.7,
            drivetrain_efficiency: 0.88,
            max_brake_force_n: 11000.0,
            drag_area_m2: 0.62,
            downforce_area_m2: 0.1,
            rolling_resistance: 0.012,
            tyre_grip: 1.05,
            max_steer_angle_rad: 0.55,
            steer_falloff_speed_mps: 42.0,
            max_speed_mps: 62.0,
            front_drive_bias: 0.0,
            steer_rate_per_s: 5.0,
            steer_return_per_s: 7.0,
            lateral_grip_per_s: 7.0,
        }
    }
}

impl ArcadeConfig {
    /// Check ranges; the model assumes a validated configuration.
    pub fn validate(&self) -> Result<(), &'static str> {
        let positive = [
            self.mass_kg,
            self.wheelbase_m,
            self.wheel_radius_m,
            self.peak_torque_nm,
            self.idle_rpm,
            self.redline_rpm,
            self.reverse_ratio,
            self.final_drive,
            self.max_brake_force_n,
            self.tyre_grip,
            self.max_steer_angle_rad,
            self.steer_falloff_speed_mps,
            self.max_speed_mps,
            self.steer_rate_per_s,
            self.steer_return_per_s,
            self.lateral_grip_per_s,
        ];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err("arcade parameters must be finite and positive");
        }
        let non_negative = [
            self.drag_area_m2,
            self.downforce_area_m2,
            self.rolling_resistance,
        ];
        if non_negative.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err("arcade drag and rolling parameters must be finite and non-negative");
        }
        if self.redline_rpm <= self.idle_rpm {
            return Err("arcade redline must exceed idle");
        }
        if self.gear_ratios.is_empty() || self.gear_ratios.len() > ARCADE_MAX_GEARS {
            return Err("arcade gear count must be 1..=12");
        }
        if self.gear_ratios.iter().any(|g| !g.is_finite() || *g <= 0.0)
            || self.gear_ratios.windows(2).any(|w| w[1] > w[0])
        {
            return Err("arcade gear ratios must be positive and non-increasing");
        }
        if !(0.01..=1.0).contains(&self.drivetrain_efficiency)
            || !(0.0..=1.0).contains(&self.front_drive_bias)
            || self.max_steer_angle_rad > 1.2
            || self.lateral_grip_per_s < 1.0
            || self.lateral_grip_per_s > 40.0
            || self.steer_rate_per_s > 50.0
            || self.steer_return_per_s > 50.0
            || self.max_speed_mps > 200.0
        {
            return Err("arcade parameter outside its supported range");
        }
        Ok(())
    }

    /// Bit patterns of every parameter, for world identity hashing.
    pub fn identity_words(&self) -> Vec<u64> {
        let mut words: Vec<u64> = [
            self.mass_kg,
            self.wheelbase_m,
            self.wheel_radius_m,
            self.peak_torque_nm,
            self.idle_rpm,
            self.redline_rpm,
            self.reverse_ratio,
            self.final_drive,
            self.drivetrain_efficiency,
            self.max_brake_force_n,
            self.drag_area_m2,
            self.downforce_area_m2,
            self.rolling_resistance,
            self.tyre_grip,
            self.max_steer_angle_rad,
            self.steer_falloff_speed_mps,
            self.max_speed_mps,
            self.front_drive_bias,
            self.steer_rate_per_s,
            self.steer_return_per_s,
            self.lateral_grip_per_s,
        ]
        .iter()
        .map(|v| v.to_bits())
        .collect();
        words.push(self.gear_ratios.len() as u64);
        words.extend(self.gear_ratios.iter().map(|v| v.to_bits()));
        words
    }

    /// Highest forward gear number.
    pub fn gear_count(&self) -> u8 {
        self.gear_ratios.len().min(ARCADE_MAX_GEARS) as u8
    }

    fn ratio(&self, gear: u8) -> f64 {
        let index = usize::from(gear.max(1)) - 1;
        self.gear_ratios[index.min(self.gear_ratios.len() - 1)]
    }

    /// Engine speed for a road speed in a given overall ratio (rpm).
    fn rpm_for(&self, speed: f64, ratio: f64) -> f64 {
        speed / self.wheel_radius_m * (60.0 / TAU) * ratio * self.final_drive
    }

    /// Engine torque factor over the rev range: 0.65 at idle and redline,
    /// 1.0 mid-range, fading to nothing at the limiter.
    fn torque_factor(&self, rpm: f64) -> f64 {
        let x = ((rpm - self.idle_rpm) / (self.redline_rpm - self.idle_rpm)).max(0.0);
        if x >= 1.0 {
            return 0.0;
        }
        let base = 0.65 + 1.4 * x * (1.0 - x);
        base * ((1.0 - x) / 0.03).min(1.0)
    }

    /// Grip-limited force available at the tyres at `speed`, newtons.
    fn grip_force(&self, speed: f64) -> f64 {
        let downforce = 0.5 * RHO * self.downforce_area_m2 * speed * speed;
        self.tyre_grip * (self.mass_kg * G + downforce)
    }
}

fn approach(current: f64, target: f64, max_delta: f64) -> f64 {
    let d = target - current;
    if d > max_delta {
        current + max_delta
    } else if d < -max_delta {
        current - max_delta
    } else {
        target
    }
}

/// Reduce `speed` (signed) towards zero by `amount` (>= 0) without crossing it.
fn towards_zero(speed: f64, amount: f64) -> f64 {
    if speed > 0.0 {
        (speed - amount).max(0.0)
    } else {
        (speed + amount).min(0.0)
    }
}

/// Advance one fixed step.
pub(crate) fn advance(
    cfg: &ArcadeConfig,
    vehicle: Vehicle,
    input: DriverInput,
    dt: f64,
    drive: &mut DriveState,
) -> KinematicUpdate {
    let direction = Vec2::new(vehicle.heading.cos(), vehicle.heading.sin());
    let left = Vec2::new(-direction.y, direction.x);
    let mut v_f = vehicle.velocity.dot(direction);
    let mut v_s = vehicle.velocity.dot(left);

    let gear_count = cfg.gear_count();
    let input = apply_auto_reverse(drive, input, v_f);
    if update_selector(drive, &input, gear_count).forward_shift {
        drive.shift_timer = MANUAL_SHIFT_SECONDS;
    }

    // Pedal and steering travel.
    drive.throttle_command = COMMAND_FULL
        * approach(
            drive.throttle_command / COMMAND_FULL,
            input.throttle,
            if input.throttle > drive.throttle_command / COMMAND_FULL {
                THROTTLE_RISE
            } else {
                THROTTLE_FALL
            } * dt,
        );
    drive.brake_command = COMMAND_FULL
        * approach(
            drive.brake_command / COMMAND_FULL,
            input.brake,
            BRAKE_RATE * dt,
        );
    let steer_now = drive.steer_command / STEER_FULL;
    let toward_centre = input.steer.abs() < steer_now.abs() || input.steer * steer_now < 0.0;
    let steer_rate = if toward_centre {
        cfg.steer_return_per_s
    } else {
        cfg.steer_rate_per_s
    };
    let steer = approach(steer_now, input.steer, steer_rate * dt);
    drive.steer_command = steer * STEER_FULL;
    let throttle = drive.throttle_command / COMMAND_FULL;
    let brake = drive.brake_command / COMMAND_FULL;

    let handbrake = input.handbrake;
    drive.handbrake_seconds = if handbrake {
        drive.handbrake_seconds + dt
    } else {
        0.0
    };

    // Automatic gearbox with hysteresis between the up and down thresholds.
    let speed_abs = v_f.abs();
    drive.gear = drive.gear.clamp(1, gear_count);
    if drive.selector == SELECTOR_DRIVE && !drive.manual {
        let range = cfg.redline_rpm - cfg.idle_rpm;
        let previous = drive.gear;
        let mut gear = drive.gear;
        while gear < gear_count
            && cfg.rpm_for(speed_abs, cfg.ratio(gear)) > cfg.idle_rpm + UPSHIFT_RANGE * range
        {
            gear += 1;
        }
        while gear > 1
            && cfg.rpm_for(speed_abs, cfg.ratio(gear - 1)) < cfg.idle_rpm + DOWNSHIFT_RANGE * range
        {
            gear -= 1;
        }
        if gear != previous && brake < 0.5 {
            drive.shift_timer = SHIFT_SECONDS;
        }
        drive.gear = gear;
    } else if drive.selector == SELECTOR_NEUTRAL && speed_abs < 0.5 {
        drive.gear = 1;
    }
    let engaged = drive.shift_timer <= 0.0;
    drive.shift_timer = (drive.shift_timer - dt).max(0.0);

    // Forces along the body axis.
    let speed = vehicle.velocity.length();
    let grip_force = cfg.grip_force(speed);
    let bias_spread = (2.0 * cfg.front_drive_bias - 1.0).abs();
    let traction_share = 0.8 + 0.2 * (1.0 - bias_spread);
    let mut drive_force = 0.0;
    if engaged && (drive.selector == SELECTOR_DRIVE || drive.selector == SELECTOR_REVERSE) {
        let (ratio, sign, limit) = if drive.selector == SELECTOR_DRIVE {
            (cfg.ratio(drive.gear), 1.0, cfg.max_speed_mps)
        } else {
            (
                cfg.reverse_ratio.abs(),
                -1.0,
                cfg.max_speed_mps * REVERSE_SPEED_FRACTION,
            )
        };
        let launch = cfg.idle_rpm + LAUNCH_RANGE * (cfg.redline_rpm - cfg.idle_rpm);
        let rpm = cfg.rpm_for(speed_abs, ratio).max(launch);
        let torque = cfg.peak_torque_nm * cfg.torque_factor(rpm);
        let wheel_force = throttle * torque * ratio * cfg.final_drive * cfg.drivetrain_efficiency
            / cfg.wheel_radius_m;
        let along = v_f * sign;
        let mut governor = if along < 0.0 {
            1.0
        } else {
            ((limit - along) / (0.03 * limit)).clamp(0.0, 1.0)
        };
        if drive.manual && along >= 0.0 {
            // Rev limit: the gear holds its speed at the limit, no cycling.
            let hold = REV_HOLD_RPM_FRACTION * cfg.redline_rpm;
            governor = governor.min(((hold - rpm) / (0.02 * cfg.redline_rpm)).clamp(0.0, 1.0));
        }
        let cap = grip_force * traction_share;
        drive_force = sign * (wheel_force * governor).min(cap);
    }
    v_f += drive_force / cfg.mass_kg * dt;
    if drive.selector == SELECTOR_REVERSE && v_f > 0.0 {
        // Reverse selected while rolling forward: the driven wheels brake hard
        // against the motion, within what the tyres hold.
        let decel = REVERSE_BRAKE_DECEL.min(grip_force / cfg.mass_kg);
        v_f = towards_zero(v_f, decel * dt);
    }
    if drive.selector == SELECTOR_DRIVE
        && cfg.rpm_for(speed_abs, cfg.ratio(drive.gear)) > cfg.redline_rpm
    {
        // A gear too low for the road speed over-revs the engine to the
        // redline and its braking pulls the car down to the gear's speed.
        v_f = towards_zero(v_f, OVERREV_BRAKE_DECEL * dt);
    }

    // Resistance cannot reverse the motion; neither can the brakes.
    let aero = 0.5 * RHO * cfg.drag_area_m2 * speed * v_f.abs();
    let rolling = cfg.rolling_resistance * cfg.mass_kg * G * (v_f.abs() / 0.5).min(1.0);
    v_f = towards_zero(v_f, (aero + rolling) / cfg.mass_kg * dt);
    let mut brake_force = (brake * cfg.max_brake_force_n).min(grip_force);
    if handbrake {
        brake_force += HANDBRAKE_DECEL_G * cfg.mass_kg * G;
    }
    v_f = towards_zero(v_f, brake_force.min(grip_force) / cfg.mass_kg * dt);

    // Lateral tyre response: sideways velocity relaxes at a finite rate, up to
    // the grip limit. The handbrake loosens the rear and shortens the limit.
    let mut a_max = CORNER_GRIP_GAIN * grip_force / cfg.mass_kg;
    let mut grip_rate = cfg.lateral_grip_per_s;
    if handbrake {
        a_max *= HANDBRAKE_AMAX;
        grip_rate *= HANDBRAKE_GRIP;
    }
    let relaxed = -v_s * (grip_rate * dt) / (1.0 + grip_rate * dt);
    v_s += relaxed.clamp(-a_max * dt, a_max * dt);

    // Yaw: speed-sensitive lock, limited to what the tyres can hold, then a
    // short lag so the body follows the steering a moment after the wheels.
    let mut lock = cfg.max_steer_angle_rad / (1.0 + speed_abs / cfg.steer_falloff_speed_mps);
    if handbrake {
        lock = (lock * HANDBRAKE_YAW).min(1.2);
    }
    let kinematic = v_f * lock / cfg.wheelbase_m;
    let grip_limit = 0.95 * a_max / speed_abs.max(1.0);
    let full_rate = kinematic.clamp(-grip_limit, grip_limit);
    let target_yaw = (steer * full_rate).clamp(-MAX_YAW_RATE, MAX_YAW_RATE);
    let yaw =
        vehicle.angular_velocity + (target_yaw - vehicle.angular_velocity) * dt / (YAW_TAU + dt);

    let heading = (vehicle.heading + yaw * dt).rem_euclid(TAU);
    let velocity = direction * v_f + left * v_s;
    drive.slide_offset = (-v_s / v_f.abs().max(2.0)).clamp(-LEAD_MAX, LEAD_MAX);
    KinematicUpdate {
        heading,
        angular_velocity: yaw,
        velocity,
        proposed_position: vehicle.position + velocity * dt,
    }
}

/// Gear and engine speed for HUD and audio.
pub fn telemetry(cfg: &ArcadeConfig, drive: &DriveState, speed: f64) -> DriveTelemetry {
    let throttle = (drive.throttle_command / COMMAND_FULL).clamp(0.0, 1.0);
    let range = cfg.redline_rpm - cfg.idle_rpm;
    let (gear, rpm) = match drive.selector {
        SELECTOR_REVERSE => (
            GEAR_CODE_REVERSE,
            cfg.rpm_for(speed, cfg.reverse_ratio.abs())
                .max(cfg.idle_rpm + 0.3 * range * throttle),
        ),
        SELECTOR_NEUTRAL => (GEAR_CODE_NEUTRAL, cfg.idle_rpm + 0.85 * range * throttle),
        _ => (
            drive.gear.clamp(1, cfg.gear_count()),
            cfg.rpm_for(speed, cfg.ratio(drive.gear))
                .max(cfg.idle_rpm + 0.3 * range * throttle),
        ),
    };
    DriveTelemetry {
        gear,
        rpm01: (rpm / cfg.redline_rpm).clamp(0.0, 1.0),
    }
}
