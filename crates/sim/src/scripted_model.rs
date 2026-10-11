//! Servo-driven motion for scripted (computer-controlled) cars.
//!
//! A scripted car has no engine, gearbox, tyre or load-transfer model and no
//! input lanes. A driver module supplies three accelerations per step (along
//! the heading, sideways and about the vertical axis); this module turns them
//! into a velocity, heading and proposed position that the shared pipeline
//! (ground support, walls, bounds, checkpoints, contacts) then integrates.
//! The numbers are project-owned.

use std::f64::consts::TAU;

use crate::provisional_model::KinematicUpdate;
use crate::{Vec2, Vehicle};

/// Revision of the scripted motion equations; bound into world identity.
pub const SCRIPTED_MODEL_REVISION: &str = "scripted-servo-v1";

/// Per-step command from a driver module.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptedControl {
    /// Acceleration along the heading, metres per second squared (negative brakes).
    pub forward_accel: f64,
    /// Acceleration to the left of the heading, metres per second squared.
    pub lateral_accel: f64,
    /// Yaw acceleration, radians per second squared (positive turns left).
    pub yaw_accel: f64,
}

impl ScriptedControl {
    pub fn is_finite(&self) -> bool {
        self.forward_accel.is_finite()
            && self.lateral_accel.is_finite()
            && self.yaw_accel.is_finite()
    }
}

/// Rolling drag fraction per second (same order as the player's models).
const ROLLING_DRAG_PER_SECOND: f64 = 0.02;
/// Sideways velocity that the tyres remove per second (1/s) when the driver
/// asks for nothing; keeps un-commanded drift bounded.
const SIDE_GRIP_PER_SECOND: f64 = 2.0;
/// Yaw damping per second when no yaw acceleration is commanded.
const YAW_DAMPING_PER_SECOND: f64 = 3.0;
const MAX_YAW_RATE: f64 = 6.0;
const MAX_SPEED: f64 = 400.0;

pub(crate) fn advance(vehicle: Vehicle, control: ScriptedControl, dt: f64) -> KinematicUpdate {
    let control = if control.is_finite() {
        control
    } else {
        ScriptedControl::default()
    };
    let dir = Vec2::new(vehicle.heading.cos(), vehicle.heading.sin());
    let left = Vec2::new(-dir.y, dir.x);
    let mut v_forward = vehicle.velocity.dot(dir);
    let mut v_side = vehicle.velocity.dot(left);
    v_forward += control.forward_accel * dt;
    v_side += control.lateral_accel * dt;
    if control.lateral_accel == 0.0 {
        v_side *= (-SIDE_GRIP_PER_SECOND * dt).exp();
    }
    v_forward *= 1.0 - (ROLLING_DRAG_PER_SECOND * dt).min(0.99);
    v_forward = v_forward.clamp(-MAX_SPEED, MAX_SPEED);

    let mut omega = vehicle.angular_velocity + control.yaw_accel * dt;
    if control.yaw_accel == 0.0 {
        omega *= (-YAW_DAMPING_PER_SECOND * dt).exp();
    }
    omega = omega.clamp(-MAX_YAW_RATE, MAX_YAW_RATE);
    let heading = (vehicle.heading + omega * dt).rem_euclid(TAU);

    let new_dir = Vec2::new(heading.cos(), heading.sin());
    let new_left = Vec2::new(-new_dir.y, new_dir.x);
    let velocity = new_dir * v_forward + new_left * v_side;
    KinematicUpdate {
        heading,
        angular_velocity: omega,
        velocity,
        proposed_position: vehicle.position + velocity * dt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DriverInput, RaceWorld, VehicleModel};

    fn world() -> RaceWorld {
        let mut w = RaceWorld::default();
        w.set_vehicle_model(VehicleModel::Scripted).unwrap();
        w
    }

    #[test]
    fn commanded_accelerations_move_turn_and_slide_the_car() {
        let mut w = world();
        w.set_scripted_control(ScriptedControl {
            forward_accel: 6.0,
            lateral_accel: 0.0,
            yaw_accel: 0.0,
        });
        for _ in 0..120 {
            w.step(DriverInput::default());
        }
        let speed = w.vehicle.speed();
        assert!((speed - 11.4).abs() < 1.0, "{speed}");
        // A yaw command turns the heading; the sideways command moves the car sideways.
        let before = w.vehicle.heading;
        w.set_scripted_control(ScriptedControl {
            forward_accel: 0.0,
            lateral_accel: 4.0,
            yaw_accel: 3.0,
        });
        for _ in 0..30 {
            w.step(DriverInput::default());
        }
        assert!(w.vehicle.heading != before);
        assert!(w.vehicle.angular_velocity > 0.0);
    }

    #[test]
    fn without_commands_drift_and_spin_decay_and_bad_commands_are_ignored() {
        let mut w = world();
        w.set_scripted_control(ScriptedControl {
            forward_accel: 0.0,
            lateral_accel: 6.0,
            yaw_accel: 6.0,
        });
        for _ in 0..30 {
            w.step(DriverInput::default());
        }
        w.set_scripted_control(ScriptedControl::default());
        for _ in 0..300 {
            w.step(DriverInput::default());
        }
        assert!(w.vehicle.angular_velocity.abs() < 1.0e-3);
        w.set_scripted_control(ScriptedControl {
            forward_accel: f64::NAN,
            lateral_accel: f64::INFINITY,
            yaw_accel: 1.0,
        });
        w.step(DriverInput::default());
        assert!(w.vehicle.position.x.is_finite() && w.vehicle.velocity.x.is_finite());
    }

    #[test]
    fn scripted_worlds_have_their_own_identity_and_replay_exactly() {
        let a = world();
        let b = RaceWorld::default();
        assert_ne!(
            a.simulation_identity().unwrap(),
            b.simulation_identity().unwrap()
        );
        let run = || {
            let mut w = world();
            for tick in 0..200 {
                w.set_scripted_control(ScriptedControl {
                    forward_accel: 3.0,
                    lateral_accel: 0.0,
                    yaw_accel: if tick % 40 < 20 { 1.0 } else { -1.0 },
                });
                w.step(DriverInput::default());
            }
            (w.vehicle.position, w.vehicle.heading)
        };
        assert_eq!(run(), run());
    }
}
