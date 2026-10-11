//! Road-edge wall contact with the classic model, against the wall-contact
//! targets of `tests/feel/classic-targets.json` on a fabricated straight road.
//! The wall law itself is model independent (`racer_sim::track_edge`); these
//! tests check that a classic car feels it as tuned: no bounce, a speed loss
//! that follows the contact friction, a heading that turns toward the wall and
//! a car that drives away along the wall afterwards.

use racer_sim::track_edge::{
    box_extent, BOX_HALF_LENGTH_M, BOX_HALF_WIDTH_M, WALL_FRICTION, WALL_HEADING_GAIN,
};
use racer_sim::{
    ClassicConfig, DriverInput, PreviewCollisionWorld, PreviewGroundTriangle, RaceWorld, SimTiming,
    TrackEdgeRow, TrackRuntime, Vec2,
};
use serde_json::Value;

const LEFT: f64 = 6.0;
const RIGHT: f64 = 6.0;

fn wall_targets() -> Value {
    let all: Value =
        serde_json::from_str(include_str!("../../../tests/feel/classic-targets.json")).unwrap();
    all["wall_contact"].clone()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn ground(extent: f64) -> Vec<PreviewGroundTriangle> {
    vec![
        PreviewGroundTriangle {
            vertices: [
                [-extent, -extent, 0.0],
                [extent, -extent, 0.0],
                [extent, extent, 0.0],
            ],
        },
        PreviewGroundTriangle {
            vertices: [
                [-extent, -extent, 0.0],
                [extent, extent, 0.0],
                [-extent, extent, 0.0],
            ],
        },
    ]
}

/// Straight road along +X; the right vector points to -Y.
fn straight_rows() -> Vec<TrackEdgeRow> {
    (0..400)
        .map(|i| TrackEdgeRow {
            x: i as f64 * 10.0,
            y: 0.0,
            z: 0.0,
            forward: Vec2::new(1.0, 0.0),
            right: Vec2::new(0.0, -1.0),
            left_distance: LEFT,
            right_distance: RIGHT,
            block: (i / 8) as u16,
        })
        .collect()
}

fn world(hz: u32) -> RaceWorld {
    let timing = SimTiming {
        physics_hz: hz,
        input_hz: hz,
        race_clock_hz: hz,
    };
    let mut world = RaceWorld::new(timing, TrackRuntime::open_preview());
    world.use_classic_model(ClassicConfig::default()).unwrap();
    world
        .install_preview_collision(PreviewCollisionWorld {
            ground: ground(5000.0),
            walls: Vec::new(),
            vehicle_radius: 1.0,
            vehicle_min_z: -0.5,
            vehicle_max_z: 0.5,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: straight_rows(),
        })
        .unwrap();
    world
}

fn place(world: &mut RaceWorld, heading: f64, speed: f64) {
    world.vehicle.position = Vec2::new(100.0, 0.0);
    world.vehicle.heading = heading;
    world.vehicle.velocity = Vec2::new(heading.cos(), heading.sin()) * speed;
}

fn full_throttle() -> DriverInput {
    DriverInput {
        throttle: 1.0,
        ..DriverInput::default()
    }
}

/// Step once; true when the heading changed (a contact tick; no steering).
fn step_contact(world: &mut RaceWorld) -> bool {
    let before = world.vehicle.heading;
    world.step(full_throttle());
    (world.vehicle.heading - before).abs() > 1.0e-9
}

#[test]
fn the_wall_constants_are_the_tuned_targets() {
    let t = wall_targets();
    assert_eq!(BOX_HALF_WIDTH_M, f(&t["car_half_width_m"]));
    assert_eq!(BOX_HALF_LENGTH_M, f(&t["car_half_length_m"]));
    assert_eq!(WALL_FRICTION, f(&t["friction"]));
    assert_eq!(WALL_HEADING_GAIN, f(&t["heading_gain_per_tick"]));
}

#[test]
fn a_shallow_hit_loses_speed_in_the_target_sequence_without_bouncing() {
    let t = wall_targets();
    let hit = &t["shallow_hit"];
    let (theta, v0) = (f(&hit["angle_rad"]), f(&hit["speed_mps"]));
    let expected: Vec<f64> = hit["speeds_after_hits_mps"]
        .as_array()
        .unwrap()
        .iter()
        .map(f)
        .collect();
    let mut w = world(32);
    place(&mut w, theta, v0);
    let mut speeds = Vec::new();
    for _ in 0..40 {
        if step_contact(&mut w) {
            speeds.push(w.vehicle.speed());
        }
        // No bounce: the car never moves away from the wall faster than its
        // heading allows, and never gets outside the box limit.
        assert!(w.vehicle.position.y <= LEFT - BOX_HALF_WIDTH_M + 1.0e-9);
    }
    assert!(speeds.len() >= expected.len(), "{speeds:?}");
    let ratio = speeds[0] / v0;
    let model = 1.0 - theta.sin();
    assert!(
        (ratio - model).abs() < f(&hit["first_hit_ratio"]["tolerance"]),
        "first hit ratio {ratio} vs {model}"
    );
    for (index, want) in expected.iter().enumerate() {
        assert!(
            (speeds[index] - want).abs() <= f(&hit["tolerance_mps"]),
            "contact {}: {:.2} vs {want}",
            index + 1,
            speeds[index]
        );
    }
    assert!(speeds[..5].windows(2).all(|pair| pair[1] < pair[0] + 0.5));
}

#[test]
fn the_hit_is_step_rate_independent_in_time() {
    let t = wall_targets();
    let hit = &t["shallow_hit"];
    let mut w = world(60);
    place(&mut w, f(&hit["angle_rad"]), f(&hit["speed_mps"]));
    let mut first = None;
    let mut sample = None;
    for step in 0..60usize {
        let contact = step_contact(&mut w);
        if first.is_none() && contact {
            first = Some(step);
        }
        // Four tuning ticks (7 steps at 60 Hz) after the first impact.
        if first.is_some_and(|first| step == first + 7) {
            sample = Some(w.vehicle.speed());
        }
    }
    let want = f(&hit["speeds_after_hits_mps"][3]);
    assert!(
        (sample.unwrap() - want).abs() <= 2.2,
        "{sample:?} vs {want}"
    );
}

#[test]
fn contact_rests_the_box_at_the_edge_minus_its_extent_and_restitution_is_zero() {
    assert_eq!(f(&wall_targets()["restitution"]), 0.0);
    for theta in [0.1f64, 0.33, 0.7, 1.05] {
        let mut w = world(32);
        place(&mut w, theta, 30.0);
        let mut contact_y = None;
        for _ in 0..60 {
            if step_contact(&mut w) {
                contact_y = Some(w.vehicle.position.y);
                break;
            }
        }
        let want = LEFT - box_extent(BOX_HALF_WIDTH_M, BOX_HALF_LENGTH_M, theta);
        assert!((contact_y.unwrap() - want).abs() < 1.0e-9, "theta {theta}");
    }
}

#[test]
fn a_head_on_hit_stops_and_the_car_drives_away_along_the_wall() {
    let t = wall_targets();
    assert!(t["head_on_stops"].as_bool().unwrap());
    let mut w = world(32);
    place(&mut w, 1.05, 36.0);
    let mut stopped = false;
    for _ in 0..40 {
        if step_contact(&mut w) {
            assert!(w.vehicle.speed() < 1.0e-6, "{}", w.vehicle.speed());
            stopped = true;
            break;
        }
    }
    assert!(stopped);
    let stop_x = w.vehicle.position.x;
    for _ in 0..400 {
        w.step(full_throttle());
    }
    assert!(w.vehicle.position.x > stop_x + 100.0);
    assert!(w.vehicle.speed() > 10.0);
}

#[test]
fn a_parallel_slide_rests_inside_the_edge() {
    let t = wall_targets();
    let gap = &t["rest_gap_m"];
    for (heading, wall_y) in [(0.2, LEFT), (-0.2, -RIGHT)] {
        let mut w = world(32);
        place(&mut w, heading, 35.0);
        for _ in 0..160 {
            w.step(full_throttle());
        }
        let inside = wall_y.abs() - w.vehicle.position.y.abs();
        assert!(
            inside >= f(&gap[0]) - 1.0e-9 && inside <= f(&gap[1]),
            "rest offset {inside}"
        );
        assert!(w.vehicle.heading.sin().abs() < 0.03);
        assert!(w.vehicle.speed() > 10.0, "slides on at speed");
    }
}
