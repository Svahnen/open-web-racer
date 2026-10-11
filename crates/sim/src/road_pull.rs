//! Downhill pull on a side-sloped (cambered) road.
//!
//! A car driven hands-off on a strongly tilted road turns toward the downhill
//! side by itself at low speed. This module holds the project's fit of that
//! behaviour (observation spec `tests/feel`, targets
//! `tests/feel/road-pull-targets.json`). It is a pure function
//! of the road side slope, the car's speed and throttle and the car's full-lock
//! yaw rate at that speed, shared by the project Classic model and the
//! compatibility models so that they agree.
//!
//! The fit, with `s` the signed side slope (sine of the road roll angle, positive
//! when the downhill side is on the car's left, i.e. the pull is counter-clockwise):
//!
//! * a slope of at least [`SLOPE_THRESHOLD`] pulls like a steering command of
//!   [`PULL_GAIN`] `* s` of full lock, so the yaw rate is `PULL_GAIN * s * W(v)`
//!   with `W` the car's full-lock yaw curve (it keeps its knee);
//! * the pull exists only below a gate speed that falls from [`GATE_COAST_MPS`] with
//!   the throttle off to [`GATE_THROTTLE_MPS`] with the throttle full, fading over
//!   [`GATE_FADE_MPS`];
//! * everywhere else a small residual yaw rate of [`RESIDUAL_YAW_PER_SLOPE`]` * s` remains.

/// Side slope (sine of the roll angle) from which the pull acts.
pub const SLOPE_THRESHOLD: f64 = 0.285;
/// Pull as a fraction of the full-lock command per unit of side slope.
pub const PULL_GAIN: f64 = 0.233;
/// Residual yaw rate (rad/s) per unit of side slope below the threshold or the gate.
pub const RESIDUAL_YAW_PER_SLOPE: f64 = 0.05;
/// Gate speed (m/s) with the throttle off.
pub const GATE_COAST_MPS: f64 = 25.2;
/// Gate speed (m/s) with the throttle full: the steering knee speed.
pub const GATE_THROTTLE_MPS: f64 = 20.5;
/// Width (m/s) below the gate over which the pull builds up.
pub const GATE_FADE_MPS: f64 = 0.8;
/// The residual yaw rate builds up with speed over this range (m/s).
const RESIDUAL_SPEED_RAMP_MPS: f64 = 2.0;
/// The pull builds up with speed and is complete at this speed (m/s).
const PULL_AUTHORITY_MPS: f64 = 5.5;
/// Speed (m/s) at which the steering curves of the models reach full authority;
/// the `full_lock_yaw` passed in already includes that ramp, which the pull
/// replaces by its own.
const STEER_AUTHORITY_MPS: f64 = 8.0;

/// Gate speed (m/s) for a throttle fraction (0..1).
pub fn gate_speed(throttle01: f64) -> f64 {
    let t = if throttle01.is_finite() {
        throttle01.clamp(0.0, 1.0)
    } else {
        0.0
    };
    GATE_COAST_MPS + (GATE_THROTTLE_MPS - GATE_COAST_MPS) * t
}

/// Extra yaw rate (rad/s, counter-clockwise positive) of a car moving forward at
/// `speed` (m/s) on a road with side slope `slope` (positive: downhill on the
/// left) with `throttle01` of throttle and a full-lock yaw rate `full_lock_yaw`
/// (rad/s, magnitude) at this speed.
pub fn slope_yaw_rate(slope: f64, speed: f64, throttle01: f64, full_lock_yaw: f64) -> f64 {
    if !slope.is_finite() || !speed.is_finite() || !full_lock_yaw.is_finite() {
        return 0.0;
    }
    let s = slope.clamp(-1.0, 1.0);
    let speed = speed.max(0.0);
    let residual = RESIDUAL_YAW_PER_SLOPE * s * (speed / RESIDUAL_SPEED_RAMP_MPS).clamp(0.0, 1.0);
    if s.abs() < SLOPE_THRESHOLD {
        return residual;
    }
    if speed < 1.0e-6 {
        return 0.0;
    }
    let weight = ((gate_speed(throttle01) - speed) / GATE_FADE_MPS).clamp(0.0, 1.0);
    let ramp = (speed / PULL_AUTHORITY_MPS).clamp(0.0, 1.0)
        / (speed / STEER_AUTHORITY_MPS).clamp(0.0, 1.0);
    weight * PULL_GAIN * s * full_lock_yaw * ramp + (1.0 - weight) * residual
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_road_adds_nothing() {
        for speed in [0.0, 5.0, 20.0, 40.0] {
            assert_eq!(slope_yaw_rate(0.0, speed, 0.0, 1.7), 0.0);
        }
    }

    #[test]
    fn pull_is_antisymmetric_in_the_slope() {
        let a = slope_yaw_rate(0.4, 10.0, 0.0, 1.7);
        let b = slope_yaw_rate(-0.4, 10.0, 0.0, 1.7);
        assert!(a > 0.0 && (a + b).abs() < 1e-12);
    }

    #[test]
    fn gate_falls_with_throttle() {
        assert!(gate_speed(0.0) > gate_speed(0.5) && gate_speed(0.5) > gate_speed(1.0));
        assert_eq!(gate_speed(f64::NAN), GATE_COAST_MPS);
    }

    #[test]
    fn steep_slope_pulls_below_the_gate_only() {
        let low = slope_yaw_rate(0.443, 10.0, 1.0, 1.73);
        let high = slope_yaw_rate(0.443, 25.0, 1.0, 1.0);
        assert!((low - 0.233 * 0.443 * 1.73).abs() < 1e-9);
        assert!((high - RESIDUAL_YAW_PER_SLOPE * 0.443).abs() < 1e-9);
    }
}
