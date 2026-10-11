//! Existing project-owned provisional vehicle kinematics.
//!
//! This module preserves the current RaceWorld policy. Its values and behavior
//! are not a compatibility model or a claim about any commercial game.

use std::f64::consts::TAU;

use crate::{DriverInput, PreviewDrivePolicy, PreviewGripPolicy, Vec2, Vehicle};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProvisionalPolicy {
    pub use_preview_handling: bool,
    pub drive: PreviewDrivePolicy,
    pub grip: PreviewGripPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KinematicUpdate {
    pub heading: f64,
    pub angular_velocity: f64,
    pub velocity: Vec2,
    pub proposed_position: Vec2,
}

/// Advance the existing provisional heading and planar kinematics by one
/// caller-supplied timestep. Input is already normalized by RaceWorld. This
/// function does not own race ticks, reset/finish behavior, support, collision,
/// bounds, or race progression.
pub(crate) fn advance(
    vehicle: Vehicle,
    input: DriverInput,
    dt: f64,
    policy: ProvisionalPolicy,
) -> KinematicUpdate {
    let speed = vehicle.speed();
    let direction = Vec2::new(vehicle.heading.cos(), vehicle.heading.sin());
    let (forward_acceleration, brake_deceleration, max_speed) = if policy.use_preview_handling {
        (
            policy.drive.acceleration_per_second,
            policy.drive.braking_per_second,
            policy.drive.max_speed,
        )
    } else {
        (12.0, 20.0, 45.0)
    };
    let acceleration = input.throttle * forward_acceleration - input.brake * brake_deceleration;
    let mut velocity = vehicle.velocity + direction * (acceleration * dt);
    if policy.use_preview_handling {
        // Preserve the existing project-preview lateral recovery behavior.
        let left = Vec2::new(-direction.y, direction.x);
        let lateral_speed = velocity.dot(left);
        let grip_rate = if input.handbrake {
            policy.grip.handbrake_recovery_per_second
        } else {
            policy.grip.lateral_recovery_per_second
        };
        velocity = velocity - left * (lateral_speed * (1.0 - (-grip_rate * dt).exp()));
    }
    // Preserve the existing rolling drag and speed-limit operation order.
    velocity = velocity * (1.0 - (0.12 * dt).min(0.99));
    let new_speed = velocity.length();
    if new_speed > max_speed {
        velocity = velocity * (max_speed / new_speed);
    }

    // Generic worlds retain the speed-magnitude steering response. Preview
    // handling uses signed post-force body-forward speed.
    let signed_forward_speed = velocity.dot(direction);
    let gain = steering_gain(
        speed,
        signed_forward_speed,
        policy.use_preview_handling,
        input.handbrake,
    );
    let angular_velocity = input.steer * 1.8 * gain;
    let heading = (vehicle.heading + angular_velocity * dt).rem_euclid(TAU);
    let proposed_position = vehicle.position + velocity * dt;

    KinematicUpdate {
        heading,
        angular_velocity,
        velocity,
        proposed_position,
    }
}

pub(super) fn steering_gain(
    speed: f64,
    signed_forward_speed: f64,
    preview_handling: bool,
    handbrake: bool,
) -> f64 {
    let base_gain = if preview_handling {
        (signed_forward_speed / 8.0).clamp(-1.0, 1.0)
    } else {
        (speed / 8.0).clamp(0.15, 1.0)
    };
    base_gain * if handbrake { 1.5 } else { 1.0 }
}
