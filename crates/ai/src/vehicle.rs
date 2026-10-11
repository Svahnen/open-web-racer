//! Driver limits derived from a car's Arcade parameters.
//!
//! A computer car moves with the servo model (no engine), so what keeps it
//! honest is the limits its driver respects: top speed, the acceleration the
//! car really has at each speed, braking and cornering grip. They are measured
//! from the same parameters the human cars drive with, so a quick car makes a
//! quick opponent.

use racer_pack_schema::ai::VehicleAi;
use racer_sim::{ArcadeConfig, ClassicConfig, DriverInput, RaceWorld, SimTiming, TrackRuntime};

use crate::config::default_vehicle_ai;

/// Share of the car's top speed an opponent aims for at best.
const TOP_SPEED_SHARE: f64 = 0.97;
/// Share of the car's braking and grip limits the drivers use.
const LIMIT_SHARE: f64 = 0.85;
const G: f64 = 9.81;
/// Lowest acceleration a classic computer driver plans with below its cap.
const AI_ACCEL_FLOOR: f32 = 3.0;
/// Share of the classic car's grip (in g) that computer drivers corner with:
/// the servo-driven opponents follow the road with a bounded yaw acceleration,
/// so they use far less than a human at full lock.
const CLASSIC_LATERAL_SHARE: f64 = 0.3;
/// Entries of the acceleration table (whole metres per second).
const TABLE_LEN: usize = 112;

/// Acceleration (m/s^2) of the car at full throttle for each whole m/s of
/// speed, 0..=111, measured by driving the Arcade model on flat ground and
/// averaging over half a second so gear changes show as a dip rather than a
/// spike. Above the top speed the last measured value is repeated.
pub fn measured_acceleration(config: &ArcadeConfig) -> Vec<f32> {
    let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    if world.use_arcade_model(config.clone()).is_err() {
        return Vec::new();
    }
    acceleration_table(world)
}

/// The same table for a car driven with the classic model.
pub fn measured_acceleration_classic(config: &ClassicConfig) -> Vec<f32> {
    let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    if world.use_classic_model(config.clone()).is_err() {
        return Vec::new();
    }
    acceleration_table(world)
}

/// Full-throttle run of an already configured world, reduced to the table.
fn acceleration_table(mut world: RaceWorld) -> Vec<f32> {
    let dt = world.timing.step_seconds();
    let ticks = (90.0 / dt) as usize;
    let mut speeds = Vec::with_capacity(ticks + 1);
    speeds.push(0.0f64);
    for _ in 0..ticks {
        world.step(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        speeds.push(world.vehicle.velocity.length());
    }
    let half = ((0.25 / dt) as usize).max(1);
    let mut table = vec![0.0f32; TABLE_LEN];
    let mut cursor = 0usize;
    let mut last = 1.0f64;
    for (i, slot) in table.iter_mut().enumerate() {
        let target = i as f64;
        while cursor + 1 < speeds.len() && speeds[cursor] < target {
            cursor += 1;
        }
        if cursor + 1 >= speeds.len() || speeds[cursor] < target {
            *slot = last.max(0.5) as f32;
            continue;
        }
        let lo = cursor.saturating_sub(half);
        let hi = (cursor + half).min(speeds.len() - 1);
        let a = (speeds[hi] - speeds[lo]) / ((hi - lo) as f64 * dt);
        last = a.max(0.5);
        *slot = last as f32;
    }
    table
}

/// Limits of the drivers of a car with these Arcade parameters.
pub fn vehicle_ai_from_arcade(config: &ArcadeConfig) -> VehicleAi {
    let mut ai = default_vehicle_ai();
    ai.speed_cap_mps = (config.max_speed_mps * TOP_SPEED_SHARE) as f32;
    ai.acceleration_mps2 = measured_acceleration(config);
    let grip = config.tyre_grip * G;
    let brake = (config.max_brake_force_n / config.mass_kg).min(grip);
    ai.brake_mps2 = (brake * LIMIT_SHARE).clamp(3.0, 40.0) as f32;
    ai.lateral_accel_cap_mps2 = (grip * LIMIT_SHARE).clamp(4.0, 40.0) as f32;
    ai
}

/// Limits of the drivers of a car with these classic parameters. Opponents
/// brake and corner with a share of what the car can do: the braking figure is
/// the strong low-speed brake, the cornering figure follows the car's grip.
pub fn vehicle_ai_from_classic(config: &ClassicConfig) -> VehicleAi {
    let mut ai = default_vehicle_ai();
    ai.speed_cap_mps = (config.top_speed_mps * TOP_SPEED_SHARE) as f32;
    // The measured table dips during gear changes (the drive is cut); a
    // computer driver plans with a floor below its cap so a shift pause or the
    // launch delay never stalls it.
    let cap = ai.speed_cap_mps;
    ai.acceleration_mps2 = measured_acceleration_classic(config)
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            if (i as f32) < cap {
                a.max(AI_ACCEL_FLOOR)
            } else {
                a
            }
        })
        .collect();
    // Brake with a modest share of the very strong low-speed brake: the
    // generated tracks plan their braking zones for a road car.
    ai.brake_mps2 = (config.low_speed_brake_mps2() * LIMIT_SHARE * 0.4).clamp(3.0, 40.0) as f32;
    ai.lateral_accel_cap_mps2 = (config.grip * G * CLASSIC_LATERAL_SHARE).clamp(4.0, 40.0) as f32;
    ai
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_limits_follow_the_car() {
        let slow = ClassicConfig {
            top_speed_mps: 70.0,
            accel_scale: 0.6,
            ..ClassicConfig::default()
        };
        let fast = ClassicConfig::default();
        let (a, b) = (
            vehicle_ai_from_classic(&slow),
            vehicle_ai_from_classic(&fast),
        );
        assert!(b.speed_cap_mps > a.speed_cap_mps);
        assert_eq!(b.acceleration_mps2.len(), TABLE_LEN);
        assert!(b.acceleration_mps2.iter().all(|a| *a >= 0.5 && *a < 30.0));
        assert!(b.acceleration_mps2[5] > b.acceleration_mps2[55]);
        assert!(b.acceleration_mps2[5] > a.acceleration_mps2[5]);
        assert!(b.brake_mps2 > 3.0 && b.lateral_accel_cap_mps2 > 4.0);
    }

    #[test]
    fn limits_follow_the_car() {
        let slow = ArcadeConfig {
            max_speed_mps: 45.0,
            ..ArcadeConfig::default()
        };
        let fast = ArcadeConfig::default();
        let (a, b) = (vehicle_ai_from_arcade(&slow), vehicle_ai_from_arcade(&fast));
        assert!(b.speed_cap_mps > a.speed_cap_mps);
        assert_eq!(b.acceleration_mps2.len(), TABLE_LEN);
        assert!(b.acceleration_mps2.iter().all(|a| *a >= 0.5 && *a < 30.0));
        // A standing start accelerates harder than a run near top speed.
        assert!(b.acceleration_mps2[5] > b.acceleration_mps2[55]);
        assert!(b.brake_mps2 > 3.0 && b.lateral_accel_cap_mps2 > 4.0);
    }
}
