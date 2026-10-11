//! The four demo cars with the classic model on the generated tracks: a driven
//! lap per car, the road-edge wall, and the declared parameter ranges.

use racer_content::classic_params::{
    ProjectHandling, project_classic_config, use_project_handling,
};
use racer_content::project_cars::project_cars;
use racer_content::project_tracks::load_project_track;
use racer_content::rcp_track::RcpTrackScene;
use racer_sim::{DriverInput, RaceWorld, SimTiming, Vec2, VehicleModel};
use serde_json::Value;

fn targets() -> Value {
    serde_json::from_str(include_str!("../../../tests/feel/classic-targets.json")).unwrap()
}

fn world_for(track: &RcpTrackScene, car: &str, handling: ProjectHandling) -> RaceWorld {
    let mut world = RaceWorld::new(SimTiming::default(), track.runtime.clone());
    world.enable_shared_preview_handling();
    world.set_spawn(Vec2::new(0.0, 0.0));
    world
        .install_preview_collision(track.collision.clone())
        .unwrap();
    use_project_handling(&mut world, car, handling).unwrap();
    world
}

/// Pure-pursuit driver on the centre line at a share of the AI speed profile.
fn drive_lap(world: &mut RaceWorld, track: &RcpTrackScene, max_ticks: u64) -> u64 {
    let points: Vec<[f64; 2]> = track
        .collision
        .edge_rows
        .iter()
        .map(|r| [r.x, r.y])
        .collect();
    let profile = track
        .pack_track
        .ai
        .as_ref()
        .unwrap()
        .speed_profile
        .as_ref()
        .unwrap();
    let n = points.len();
    let mut nearest = 0usize;
    let mut ticks = 0;
    while ticks < max_ticks && world.vehicle.completed_laps < 1 {
        let v = world.vehicle;
        loop {
            let d = |i: usize| {
                let p = points[i % n];
                (p[0] - v.position.x).hypot(p[1] - v.position.y)
            };
            if d(nearest + 1) < d(nearest) && nearest < n * 2 {
                nearest += 1;
            } else {
                break;
            }
        }
        let speed = v.speed();
        let look = 4 + (speed / 6.0) as usize;
        let target = points[(nearest + look) % n];
        let desired = (target[1] - v.position.y).atan2(target[0] - v.position.x);
        let mut err = desired - v.heading;
        while err > std::f64::consts::PI {
            err -= std::f64::consts::TAU;
        }
        while err < -std::f64::consts::PI {
            err += std::f64::consts::TAU;
        }
        let speed_target = f64::from(profile.forward_mps[((nearest + 3) % n) / 2]) * 0.8;
        world.step(DriverInput {
            throttle: if speed < speed_target { 1.0 } else { 0.0 },
            brake: if speed > speed_target + 4.0 { 0.6 } else { 0.0 },
            steer: (err * 2.5).clamp(-1.0, 1.0),
            ..DriverInput::default()
        });
        ticks += 1;
    }
    ticks
}

#[test]
fn every_demo_car_completes_a_driven_lap_with_classic_handling() {
    let track = load_project_track("coastline-run").unwrap();
    for car in project_cars().cars() {
        let mut world = world_for(&track, &car.id, ProjectHandling::Classic);
        assert_eq!(world.vehicle_model(), VehicleModel::Classic);
        let ticks = drive_lap(&mut world, &track, 60 * 400);
        assert!(
            world.vehicle.completed_laps >= 1,
            "{}: lap not completed in {ticks} ticks (checkpoint {})",
            car.id,
            world.vehicle.checkpoint
        );
    }
}

#[test]
fn arcade_stays_selectable_as_the_fallback_handling() {
    let track = load_project_track("coastline-run").unwrap();
    let car = &project_cars().cars()[0];
    let mut world = world_for(&track, &car.id, ProjectHandling::Arcade);
    assert_eq!(world.vehicle_model(), VehicleModel::Arcade);
    assert!(drive_lap(&mut world, &track, 60 * 400) > 0);
    assert!(world.vehicle.completed_laps >= 1);
    // And an unknown car id drives the default demo car in either mode.
    let world = world_for(&track, "no-such-car", ProjectHandling::Classic);
    assert_eq!(world.vehicle_model(), VehicleModel::Classic);
}

#[test]
fn a_classic_car_hits_the_road_edge_wall_and_slides_on() {
    let track = load_project_track("pine-ridge").unwrap();
    let wall = &targets()["wall_contact"];
    for car in project_cars().cars() {
        let mut world = world_for(&track, &car.id, ProjectHandling::Classic);
        // Start on a straight stretch, heading 0.2 rad toward the edge.
        let rows = &track.collision.edge_rows;
        let row = rows[3];
        let heading = row.forward.y.atan2(row.forward.x);
        let toward_right = -0.2;
        world.vehicle.position = Vec2::new(row.x, row.y) + row.right * 0.0;
        world.vehicle.heading = heading + toward_right;
        let h = world.vehicle.heading;
        world.vehicle.velocity = Vec2::new(h.cos(), h.sin()) * 30.0;
        let mut contact_ticks = 0;
        let mut previous_position = world.vehicle.position;
        for _ in 0..300 {
            let before = world.vehicle.heading;
            world.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
            if (world.vehicle.heading - before).abs() > 1e-9 {
                contact_ticks += 1;
            }
            // The wall never teleports or resets the car.
            let moved = (world.vehicle.position - previous_position).length();
            assert!(moved < 3.0, "{}: jumped {moved} m", car.id);
            previous_position = world.vehicle.position;
        }
        assert!(contact_ticks > 0, "{}: no wall contact", car.id);
        assert!(world.vehicle.speed() > 5.0, "{}: stuck at the wall", car.id);
        assert_eq!(f(&wall["restitution"]), 0.0);
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn demo_car_parameters_stay_inside_the_tuned_ranges() {
    let t = targets();
    let ranges = &t["car_ranges"];
    let within = |name: &str, value: f64| {
        let r = &ranges[name];
        assert!(
            value >= f(&r[0]) - 1e-4 && value <= f(&r[1]) + 1e-4,
            "{name}: {value} outside {r}"
        );
    };
    for car in project_cars().cars() {
        let c = project_classic_config(&car.id).expect(&car.id);
        within("steer_gain", c.steer_gain);
        within("grip", c.grip);
        within("top_speed_mps", c.top_speed_mps);
        within("first_upshift_mps", c.upshift_mps[0]);
        within("reverse_speed_mps", c.reverse_speed_mps);
        within("shift_seconds", c.shift_seconds);
        within("accel_scale", c.accel_scale);
        within("gear_count", f64::from(c.gear_count()));
        within("low_speed_yaw_rate_rad_s", c.low_speed_yaw_rate);
    }
    // The first car is the reference car of the feel targets.
    let first = project_classic_config(&project_cars().cars()[0].id).unwrap();
    assert!(
        (first.low_speed_yaw_rate - f(&t["steering"]["low_branch_rate_rad_s"])).abs() < 1e-4,
        "{first:?}"
    );
}
