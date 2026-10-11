//! Classic-model parameters of RCP vehicle entries.
//!
//! Converts the `classic` block of a vehicle definition into the simulation's
//! `ClassicConfig`. The project demo cars (`project_cars`) are the default
//! source; any RCP vehicle works the same way. A vehicle without a `classic`
//! block (or with rejected values) has no classic parameters and falls back to
//! its `arcade` block.

use crate::project_cars::project_cars;
use racer_pack_schema::vehicle::VehicleDef;
use racer_sim::ClassicConfig;

/// The classic parameters of a vehicle definition, or `None` when it has no
/// `classic` block or the values are rejected by the model.
pub fn classic_config(vehicle: &VehicleDef) -> Option<ClassicConfig> {
    let c = vehicle.classic.as_ref()?;
    let cfg = ClassicConfig {
        upshift_mps: c.upshift_mps.iter().map(|v| f64::from(*v)).collect(),
        downshift_mps: c.downshift_mps.iter().map(|v| f64::from(*v)).collect(),
        top_speed_mps: f64::from(c.top_speed_mps),
        accel_scale: f64::from(c.accel_scale),
        coast_scale: f64::from(c.coast_scale),
        brake_scale: f64::from(c.brake_scale),
        shift_seconds: f64::from(c.shift_seconds),
        reverse_speed_mps: f64::from(c.reverse_speed_mps),
        steer_gain: f64::from(c.steer_gain),
        grip: f64::from(c.grip),
        low_speed_yaw_rate: f64::from(c.low_speed_yaw_rate),
        yaw_boost_speed_mps: f64::from(c.yaw_boost_speed_mps),
    };
    cfg.validate().ok()?;
    Some(cfg)
}

/// Parameters of the demo car `car_id`.
pub fn project_classic_config(car_id: &str) -> Option<ClassicConfig> {
    classic_config(&project_cars().find(car_id)?.vehicle)
}

/// Parameters of the first demo car, the default of rooms and solo drives
/// without imported data.
pub fn default_project_classic_config() -> ClassicConfig {
    project_cars()
        .cars()
        .first()
        .and_then(|c| classic_config(&c.vehicle))
        .unwrap_or_default()
}

/// Which handling model a project car drives with. Classic is the default;
/// Arcade is the generic fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectHandling {
    #[default]
    Classic,
    Arcade,
}

/// Install the handling of demo car `car_id` (or the default demo car for an
/// unknown id) into `world` and make it the active model.
pub fn use_project_handling(
    world: &mut racer_sim::RaceWorld,
    car_id: &str,
    handling: ProjectHandling,
) -> Result<(), &'static str> {
    match handling {
        ProjectHandling::Classic => world.use_classic_model(
            project_classic_config(car_id).unwrap_or_else(default_project_classic_config),
        ),
        ProjectHandling::Arcade => world.use_arcade_model(
            crate::arcade_params::project_arcade_config(car_id)
                .unwrap_or_else(crate::arcade_params::default_project_arcade_config),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use racer_sim::{DriverInput, RaceWorld, SimTiming, TrackRuntime, VehicleModel};

    fn world(cfg: ClassicConfig) -> RaceWorld {
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.use_classic_model(cfg).unwrap();
        world
    }

    fn throttle() -> DriverInput {
        DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        }
    }

    #[test]
    fn every_demo_car_converts_and_reaches_its_declared_top_speed() {
        for car in project_cars().cars() {
            let cfg = project_classic_config(&car.id).expect(&car.id);
            let top = cfg.top_speed_mps;
            let mut world = world(cfg);
            assert_eq!(world.vehicle_model(), VehicleModel::Classic);
            let mut peak = 0.0f64;
            for _ in 0..(60 * 60) {
                world.step(throttle());
                peak = peak.max(world.vehicle.velocity.length());
            }
            assert!(peak <= top + 1e-6, "{}: {peak} > {top}", car.id);
            assert!(peak > 0.97 * top, "{}: only {peak} of {top}", car.id);
        }
    }

    #[test]
    fn the_first_demo_car_is_the_reference_car() {
        let quill = default_project_classic_config();
        let reference = ClassicConfig::default();
        assert_eq!(quill.top_speed_mps, reference.top_speed_mps);
        assert_eq!(quill.gear_count(), reference.gear_count());
        for (a, b) in quill.upshift_mps.iter().zip(&reference.upshift_mps) {
            assert!((a - b).abs() < 1e-3);
        }
        assert!((quill.low_speed_yaw_rate - reference.low_speed_yaw_rate).abs() < 1e-4);
    }

    /// Seconds to 27 m/s, the speed after 12 s and the top speed of a
    /// full-throttle run on flat ground.
    fn launch_figures(car_id: &str) -> (f64, f64, f64) {
        let mut world = world(project_classic_config(car_id).expect(car_id));
        let dt = world.timing.step_seconds();
        let (mut t27, mut at12, mut peak) = (f64::NAN, 0.0f64, 0.0f64);
        for tick in 0..(90 * 60) {
            world.step(throttle());
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
                    (fa.0 - fb.0).abs() > 0.1 || (fa.1 - fb.1).abs() > 1.0,
                    "{a} and {b} accelerate alike: {fa:?} vs {fb:?}"
                );
            }
        }
    }

    #[test]
    fn rejected_values_yield_no_config() {
        let mut v = project_cars().cars()[0].vehicle.clone();
        v.classic.as_mut().unwrap().grip = -1.0;
        assert!(classic_config(&v).is_none());
        v.classic = None;
        assert!(classic_config(&v).is_none());
    }
}
