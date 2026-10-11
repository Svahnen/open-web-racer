//! Downhill pull on a side-sloped road with the classic model, against the
//! targets in `tests/feel/road-pull-targets.json`.

use racer_sim::road_pull::{self, slope_yaw_rate};
use racer_sim::{
    ClassicConfig, DriverInput, PreviewCollisionWorld, PreviewGroundTriangle, RaceWorld, SimTiming,
    TrackRuntime, Vec2,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/feel/road-pull-targets.json")).unwrap()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// A large plane whose downhill side is on the left of a car heading along +X
/// when `slope > 0` (z falls as y grows). `slope` is the sine of the roll angle.
fn banked_ground(slope: f64) -> Vec<PreviewGroundTriangle> {
    let tan = slope / (1.0 - slope * slope).sqrt();
    let e = 5000.0;
    let z = |y: f64| -tan * y;
    vec![
        PreviewGroundTriangle {
            vertices: [[-e, -e, z(-e)], [e, -e, z(-e)], [e, e, z(e)]],
        },
        PreviewGroundTriangle {
            vertices: [[-e, -e, z(-e)], [e, e, z(e)], [-e, e, z(e)]],
        },
    ]
}

fn world(hz: u32, slope: f64) -> RaceWorld {
    let timing = SimTiming {
        physics_hz: hz,
        input_hz: hz,
        race_clock_hz: hz,
    };
    let mut world = RaceWorld::new(timing, TrackRuntime::open_preview());
    world.use_classic_model(ClassicConfig::default()).unwrap();
    world
        .install_preview_collision(PreviewCollisionWorld {
            ground: banked_ground(slope),
            walls: Vec::new(),
            vehicle_radius: 1.0,
            vehicle_min_z: -0.5,
            vehicle_max_z: 0.5,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        })
        .unwrap();
    world
}

fn place(world: &mut RaceWorld, heading: f64, speed: f64) {
    world.vehicle.position = Vec2::new(0.0, 0.0);
    world.vehicle.heading = heading;
    world.vehicle.velocity = Vec2::new(heading.cos(), heading.sin()) * speed;
}

/// Yaw rate (rad/s, counter-clockwise positive) after a few hands-off steps.
fn hands_off_yaw(hz: u32, slope: f64, heading: f64, speed: f64, throttle: f64) -> f64 {
    let mut w = world(hz, slope);
    place(&mut w, heading, speed);
    let input = DriverInput {
        throttle,
        ..DriverInput::default()
    };
    for _ in 0..(hz / 4) {
        w.step(input);
    }
    w.vehicle.angular_velocity
}

#[test]
fn the_side_slope_is_read_from_the_ground_normal() {
    let w = world(32, 0.4);
    assert!(
        (w.road_side_slope() - 0.4).abs() < 1e-9,
        "{}",
        w.road_side_slope()
    );
    let mut flipped = world(32, 0.4);
    flipped.vehicle.heading = std::f64::consts::PI;
    assert!((flipped.road_side_slope() + 0.4).abs() < 1e-9);
    assert_eq!(world(32, 0.0).road_side_slope(), 0.0);
}

#[test]
fn the_slope_sweep_matches_the_fixture() {
    let fix = fixture();
    let car = ClassicConfig::default();
    let mut checked = 0;
    for (speed, sweep) in fix["slope_sweep_default_car"].as_object().unwrap() {
        let speed: f64 = speed.parse().unwrap();
        for p in sweep["points"].as_array().unwrap() {
            let slope = num(&p["slope"]);
            // The step at the threshold falls inside one bin of the sweep.
            if (0.26..0.31).contains(&slope) {
                continue;
            }
            let want = num(&p["yaw_rad_s"]);
            let yaw = hands_off_yaw(32, slope, 0.0, speed, 0.0);
            let tol = (0.012_f64).max(0.1 * want);
            assert!(
                (yaw - want).abs() <= tol,
                "speed {speed} slope {slope}: yaw {yaw:.4} vs {want:.4}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 15, "only {checked} points checked");
    // Opposite side of the road: mirrored.
    let a = hands_off_yaw(32, 0.4, 0.0, 10.0, 0.0);
    let b = hands_off_yaw(32, 0.4, std::f64::consts::PI, 10.0, 0.0);
    assert!(a > 0.1 && b < -0.1 && (a + b).abs() < 1e-6, "{a} {b}");
    let _ = car;
}

#[test]
fn the_pull_scales_with_the_cars_own_steering_rate_and_the_other_side_agrees() {
    let fix = fixture();
    for car in fix["cars_low_speed"].as_array().unwrap() {
        let gain = num(&car["pull_gain"]);
        assert!(
            (gain / road_pull::PULL_GAIN - 1.0).abs() < 0.12,
            "car {}: {gain}",
            car["car_type_index"]
        );
    }
    let other = &fix["opposite_side"];
    let speed = num(&other["speed_mps"]);
    let slope = num(&other["slope"]);
    let cfg = ClassicConfig::default();
    let model = slope_yaw_rate(slope, speed, 0.0, cfg.full_lock_yaw_rate(speed)) / slope;
    let want = num(&other["yaw_per_unit_slope_toward_downhill"]);
    assert!((model / want - 1.0).abs() < 0.08, "{model} vs {want}");
}

#[test]
fn the_speed_and_throttle_gate_matches_the_fixture_tables() {
    let fix = fixture();
    let cfg = ClassicConfig::default();
    let tables = [
        ("coast_throttle_off", 0.0, [(24.0, 26.0), (19.0, 21.5)]),
        ("full_throttle", 1.0, [(19.0, 21.5), (19.0, 21.5)]),
    ];
    let mut checked = 0;
    for (name, throttle, skip) in tables {
        for p in fix["speed_dependence_default_car"][name]
            .as_array()
            .unwrap()
        {
            let speed = num(&p["speed_mps"]);
            if skip.iter().any(|(lo, hi)| (*lo..*hi).contains(&speed)) {
                continue;
            }
            let want = num(&p["yaw_per_unit_slope"]);
            let slope = 0.443;
            let model =
                slope_yaw_rate(slope, speed, throttle, cfg.full_lock_yaw_rate(speed)) / slope;
            assert!(
                (model - want).abs() <= (0.03_f64).max(0.15 * want),
                "{name} {speed}: model {model:.3} vs {want:.3}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 12, "only {checked} points checked");
}

#[test]
fn a_hands_off_car_turns_downhill_slowly_and_not_at_all_when_flat_or_fast() {
    // 10 m/s, throttle off, 0.44 slope: turns toward the downhill (left) side
    // at about 0.18 rad/s, independent of the step rate.
    for hz in [32, 60] {
        let yaw = hands_off_yaw(hz, 0.44, 0.0, 10.0, 0.0);
        assert!((0.15..0.21).contains(&yaw), "{hz} Hz: {yaw}");
    }
    assert_eq!(hands_off_yaw(32, 0.0, 0.0, 10.0, 0.0), 0.0);
    // A gentle slope only leaves the small residual.
    let gentle = hands_off_yaw(32, 0.2, 0.0, 10.0, 0.0);
    assert!(gentle > 0.0 && gentle < 0.02, "{gentle}");
    // Above the gate speed (full throttle: 20.5 m/s) the pull is gone.
    let fast = hands_off_yaw(32, 0.44, 0.0, 30.0, 1.0);
    assert!(fast.abs() < 0.04, "{fast}");
}

#[test]
fn the_road_gates_follow_the_throttle() {
    // 23 m/s: pulled with the throttle off, not with it full.
    let off = slope_yaw_rate(0.44, 23.0, 0.0, 1.1);
    let full = slope_yaw_rate(0.44, 23.0, 1.0, 1.1);
    assert!(off > 0.1 && full < 0.03, "{off} {full}");
}
