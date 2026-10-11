//! End-to-end behaviour of the drivers on the project test circuit: they move,
//! stay on the road, complete laps, react to blockers and recover.

use std::sync::Arc;

use racer_ai::config::Skill;
use racer_ai::field::{CarObs, Settings};
use racer_ai::race::{OPPONENT_CONTACT_RADIUS, OPPONENT_HALF_WIDTH};
use racer_ai::{AiConfig, AiField, Opponents, circuit_ai_track};
use racer_sim::{
    RaceWorld, SimTiming, TrackRuntime, Vec2, VehicleContactBody, resolve_pairwise_vehicle_contacts,
};

struct Race {
    opponents: Opponents,
    human: RaceWorld,
}

fn race(count: usize, skill: Skill, catch_up: bool, laps: u32) -> Race {
    let mut runtime = TrackRuntime::synthetic_circuit();
    runtime.laps_to_finish = laps;
    let track = Arc::new(circuit_ai_track(&runtime).unwrap());
    let settings = Settings {
        skill,
        catch_up,
        laps,
        ..Settings::default()
    };
    let opponents = Opponents::new(
        &runtime,
        track,
        Arc::new(AiConfig::default()),
        settings,
        count,
        1000,
        1,
        SimTiming::default(),
    );
    let mut human = RaceWorld::new(SimTiming::default(), runtime);
    let (pos, heading) = opponents.human_grid_slot(0, 1);
    human.place_vehicle(pos, heading);
    Race { opponents, human }
}

impl Race {
    fn tick(&mut self) {
        let obs = CarObs::from_world(1, &self.human, true, OPPONENT_HALF_WIDTH);
        self.opponents.step(&[obs]);
        // Shared contact pass (the human stays parked).
        let body = |id: u32, vehicle| VehicleContactBody {
            stable_id: id,
            vehicle,
            previous_position: vehicle.position,
            radius: OPPONENT_CONTACT_RADIUS,
            min_z: -0.5,
            max_z: 0.5,
        };
        let mut bodies: Vec<VehicleContactBody> = self
            .opponents
            .cars
            .iter()
            .map(|c| body(c.id, c.world.vehicle))
            .collect();
        bodies.push(body(1, self.human.vehicle));
        if resolve_pairwise_vehicle_contacts(&mut bodies).is_ok() {
            for b in bodies {
                if let Some(c) = self.opponents.cars.iter_mut().find(|c| c.id == b.stable_id) {
                    c.world.vehicle = b.vehicle;
                } else {
                    self.human.vehicle = b.vehicle;
                }
            }
        }
    }

    fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds * 60.0) as usize {
            self.tick();
        }
    }

    fn summary(&self) -> Vec<(u32, u32, bool, f64)> {
        self.opponents
            .cars
            .iter()
            .map(|c| {
                let v = c.world.vehicle;
                (c.id, v.completed_laps, v.finished, v.speed())
            })
            .collect()
    }
}

#[test]
fn seven_opponents_leave_the_grid_stay_in_bounds_and_finish_the_race() {
    let mut r = race(7, Skill::Advanced, true, 2);
    let start: Vec<Vec2> = r
        .opponents
        .cars
        .iter()
        .map(|c| c.world.vehicle.position)
        .collect();
    r.run(5.0);
    for (c, s) in r.opponents.cars.iter().zip(&start) {
        let moved = (c.world.vehicle.position - *s).length();
        assert!(moved > 5.0, "opponent {} moved only {moved:.1} m", c.id);
    }
    r.run(120.0);
    let summary = r.summary();
    let finished = summary.iter().filter(|s| s.2).count();
    assert!(finished >= 6, "only {finished}/7 finished: {summary:?}");
    for c in &r.opponents.cars {
        let p = c.world.vehicle.position;
        assert!(p.x.abs() <= 30.0 && p.y.abs() <= 20.0);
    }
}

#[test]
fn drivers_stay_on_the_road() {
    let mut r = race(3, Skill::Advanced, false, 3);
    let track = r.opponents.field.track().clone();
    let mut max_offset: f64 = 0.0;
    for _ in 0..(40.0 * 60.0) as usize {
        r.tick();
        for c in &r.opponents.cars {
            let p = c.world.vehicle.position;
            let row = track.locate([p.x, p.y], None);
            let rr = &track.rows[row];
            let x = (p.x - rr.position[0]) * rr.right[0] + (p.y - rr.position[1]) * rr.right[1];
            max_offset = max_offset.max(x.abs());
        }
    }
    assert!(max_offset < 7.5, "left the road by {max_offset:.2} m");
}

#[test]
fn runs_are_deterministic() {
    let run = || {
        let mut r = race(4, Skill::Beginner, true, 2);
        r.run(20.0);
        r.opponents
            .cars
            .iter()
            .map(|c| (c.world.vehicle.position.x, c.world.vehicle.position.y))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn beginners_are_not_faster_than_advanced_drivers() {
    let progress = |skill| {
        let mut r = race(2, skill, false, 9);
        r.run(30.0);
        r.opponents
            .cars
            .iter()
            .map(|c| {
                f64::from(c.world.vehicle.completed_laps) * 10.0 + c.world.vehicle.checkpoint as f64
            })
            .sum::<f64>()
    };
    assert!(progress(Skill::Advanced) >= progress(Skill::Beginner));
}

#[test]
fn a_stuck_car_reverses_for_a_bounded_time() {
    let mut runtime = TrackRuntime::synthetic_circuit();
    runtime.laps_to_finish = 2;
    let track = Arc::new(circuit_ai_track(&runtime).unwrap());
    let mut field = AiField::new(
        track,
        Arc::new(AiConfig::default()),
        Settings::default(),
        &[7],
    );
    // A car parked on the road, facing along it, that cannot move.
    let (p, h) = field.grid_slot(1, 2);
    let obs = CarObs {
        id: 7,
        position: p,
        heading: h,
        velocity: [0.0, 0.0],
        yaw_rate: 0.0,
        laps: 0,
        finished: false,
        human: false,
        half_width: 1.0,
    };
    let mut reversing = Vec::new();
    for tick in 0..900 {
        let controls = field.control(&[obs]);
        if controls[0].1.forward_accel < -1.0 {
            reversing.push(tick);
        }
    }
    let first = *reversing.first().expect("a stuck car starts reversing");
    assert!(first >= 40, "reversed too early at tick {first}");
    // Each reverse episode lasts a bounded time (14 decisions of 8 ticks) and
    // the car then tries forward again.
    let episodes = reversing.windows(2).filter(|w| w[1] - w[0] > 8).count() + 1;
    assert!(episodes >= 2, "reversing never ended: {reversing:?}");
}
