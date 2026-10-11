//! Arcade-model parameters of RCP vehicle entries.
//!
//! Converts the `arcade` block of a vehicle definition into the simulation's
//! `ArcadeConfig`. The project demo cars (`project_cars`) are the default
//! source; any RCP vehicle works the same way.

use crate::project_cars::project_cars;
use racer_pack_schema::vehicle::VehicleDef;
use racer_sim::ArcadeConfig;

/// The simulation parameters of a vehicle definition, or `None` when it has no
/// `arcade` block or the values are rejected by the model.
pub fn arcade_config(vehicle: &VehicleDef) -> Option<ArcadeConfig> {
    let a = vehicle.arcade.as_ref()?;
    let cfg = ArcadeConfig {
        mass_kg: f64::from(a.mass_kg),
        wheelbase_m: f64::from(a.wheelbase_m),
        wheel_radius_m: f64::from(a.wheel_radius_m),
        peak_torque_nm: f64::from(a.peak_torque_nm),
        idle_rpm: f64::from(a.idle_rpm),
        redline_rpm: f64::from(a.redline_rpm),
        gear_ratios: a.gear_ratios.iter().map(|g| f64::from(*g)).collect(),
        reverse_ratio: f64::from(a.reverse_ratio),
        final_drive: f64::from(a.final_drive),
        drivetrain_efficiency: f64::from(a.drivetrain_efficiency),
        max_brake_force_n: f64::from(a.max_brake_force_n),
        drag_area_m2: f64::from(a.drag_area_m2),
        downforce_area_m2: f64::from(a.downforce_area_m2),
        rolling_resistance: f64::from(a.rolling_resistance),
        tyre_grip: f64::from(a.tyre_grip),
        max_steer_angle_rad: f64::from(a.max_steer_angle_rad),
        steer_falloff_speed_mps: f64::from(a.steer_falloff_speed_mps),
        max_speed_mps: f64::from(a.max_speed_mps),
        front_drive_bias: f64::from(a.front_drive_bias),
        steer_rate_per_s: f64::from(a.steer_rate_per_s),
        steer_return_per_s: f64::from(a.steer_return_per_s),
        lateral_grip_per_s: f64::from(a.lateral_grip_per_s),
    };
    cfg.validate().ok()?;
    Some(cfg)
}

/// Parameters of the demo car `car_id`.
pub fn project_arcade_config(car_id: &str) -> Option<ArcadeConfig> {
    arcade_config(&project_cars().find(car_id)?.vehicle)
}

/// Parameters of the first demo car, the default of rooms and solo drives
/// without imported data.
pub fn default_project_arcade_config() -> ArcadeConfig {
    project_cars()
        .cars()
        .first()
        .and_then(|c| arcade_config(&c.vehicle))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use racer_sim::{DriverInput, RaceWorld, SimTiming, TrackRuntime, VehicleModel};

    #[test]
    fn every_demo_car_converts_and_drives_to_its_declared_top_speed() {
        for car in project_cars().cars() {
            let cfg = project_arcade_config(&car.id).expect(&car.id);
            let top = cfg.max_speed_mps;
            let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
            world.use_arcade_model(cfg).unwrap();
            assert_eq!(world.vehicle_model(), VehicleModel::Arcade);
            let mut peak = 0.0f64;
            for _ in 0..(60 * 60) {
                world.step(DriverInput {
                    throttle: 1.0,
                    ..DriverInput::default()
                });
                peak = peak.max(world.vehicle.velocity.length());
            }
            assert!(peak <= top + 1e-6, "{}: {peak} > {top}", car.id);
            assert!(peak > 0.95 * top, "{}: only {peak} of {top}", car.id);
        }
    }

    /// Classic-arcade steering: at full lock every demo car turns a 80 to 130 m
    /// radius at 50 m/s (or 0.9 of its top speed if lower, same lateral
    /// acceleration band), reaches lock in about 0.25 s and keeps
    /// useful yaw authority at 20 m/s.
    #[test]
    fn every_demo_car_corners_like_an_arcade_racer_at_speed() {
        for car in project_cars().cars() {
            let cfg = project_arcade_config(&car.id).expect(&car.id);
            assert!(cfg.steer_rate_per_s >= 4.0, "{}: slow steering", car.id);
            let speed = 50.0f64.min(0.9 * cfg.max_speed_mps);
            let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
            world.use_arcade_model(cfg).unwrap();
            let h = world.vehicle.heading;
            world.vehicle.velocity = racer_sim::Vec2::new(h.cos(), h.sin()) * speed;
            let coast = |steer: f64| DriverInput {
                throttle: 0.5,
                steer,
                ..DriverInput::default()
            };
            for _ in 0..30 {
                world.step(coast(0.0));
            }
            let mut quarter_second = 0.0;
            for tick in 0..90 {
                world.step(coast(1.0));
                if tick == 14 {
                    quarter_second = world.vehicle.angular_velocity;
                }
            }
            let v = world.vehicle.velocity.length();
            let yaw = world.vehicle.angular_velocity;
            let radius = v / yaw;
            assert!(
                (60.0 * (v / 50.0).powi(2)..=135.0 * (v / 50.0).powi(2)).contains(&radius),
                "{}: radius {radius} at {v} m/s",
                car.id
            );
            assert!(
                quarter_second > 0.75 * yaw,
                "{}: yaw {quarter_second} of {yaw} after 0.25 s",
                car.id
            );
        }
    }

    #[test]
    fn demo_cars_differ_in_pace_and_carry_engine_sound() {
        let mut tops: Vec<u64> = project_cars()
            .cars()
            .iter()
            .map(|c| arcade_config(&c.vehicle).unwrap().max_speed_mps.to_bits())
            .collect();
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), 4);
        for car in project_cars().cars() {
            assert!(car.vehicle.engine_sound.is_some(), "{}", car.id);
        }
    }

    /// Seconds to 27 m/s (about 100 km/h), the speed after 12 s and the
    /// top speed of a full-throttle run on flat ground.
    fn launch_figures(car_id: &str) -> (f64, f64, f64) {
        let cfg = project_arcade_config(car_id).expect(car_id);
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.use_arcade_model(cfg).unwrap();
        let dt = world.timing.step_seconds();
        let (mut t27, mut at12, mut peak) = (f64::NAN, 0.0f64, 0.0f64);
        for tick in 0..(90 * 60) {
            world.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
            let speed = world.vehicle.velocity.length();
            peak = peak.max(speed);
            if t27.is_nan() && speed >= 27.0 {
                t27 = f64::from(tick + 1) * dt;
            }
            if tick + 1 == 12 * 60 {
                at12 = speed;
            }
        }
        (t27, at12, peak)
    }

    #[test]
    fn the_four_demo_cars_have_distinct_launch_and_top_speed() {
        let figures: Vec<(String, (f64, f64, f64))> = project_cars()
            .cars()
            .iter()
            .map(|c| (c.id.clone(), launch_figures(&c.id)))
            .collect();
        for (i, (a, fa)) in figures.iter().enumerate() {
            for (b, fb) in &figures[i + 1..] {
                assert!(
                    (fa.2 - fb.2).abs() > 2.0,
                    "{a} and {b} top speeds too close: {} vs {}",
                    fa.2,
                    fb.2
                );
                assert!(
                    (fa.0 - fb.0).abs() > 0.15 || (fa.1 - fb.1).abs() > 1.0,
                    "{a} and {b} accelerate alike: {fa:?} vs {fb:?}"
                );
            }
        }
    }

    #[test]
    fn rejected_values_yield_no_config() {
        let mut v = project_cars().cars()[0].vehicle.clone();
        v.arcade.as_mut().unwrap().mass_kg = -1.0;
        assert!(arcade_config(&v).is_none());
        v.arcade = None;
        assert!(arcade_config(&v).is_none());
    }
}
