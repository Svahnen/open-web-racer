//! Decision-level behaviour of the drivers on a derived road: lane changes,
//! blockers, catch-up, difficulty and finished cars.

use std::sync::Arc;

use racer_ai::config::Skill;
use racer_ai::field::{CarObs, Settings};
use racer_ai::{AiConfig, AiField, AiTrack, circuit_ai_track};
use racer_sim::TrackRuntime;

fn track() -> Arc<AiTrack> {
    let mut runtime = TrackRuntime::synthetic_circuit();
    runtime.laps_to_finish = 2;
    Arc::new(circuit_ai_track(&runtime).unwrap())
}

/// A car on `row` in lane slot `lane`, heading along the road at `speed`.
fn car(
    track: &AiTrack,
    id: u32,
    row: usize,
    lane: i32,
    speed: f64,
    laps: u32,
    human: bool,
) -> CarObs {
    let r = &track.rows[row];
    let x = r.lane_centre(lane);
    let heading = r.heading();
    CarObs {
        id,
        position: track.point(row, x),
        heading,
        velocity: [heading.cos() * speed, heading.sin() * speed],
        yaw_rate: 0.0,
        laps,
        finished: false,
        human,
        half_width: 1.0,
    }
}

fn field(track: &Arc<AiTrack>, skill: Skill, catch_up: bool, ids: &[u32]) -> AiField {
    AiField::new(
        track.clone(),
        Arc::new(AiConfig::default()),
        Settings {
            skill,
            catch_up,
            ..Settings::default()
        },
        ids,
    )
}

#[test]
fn a_blocker_in_the_lane_moves_the_car_one_lane_at_a_time() {
    let t = track();
    // Row 20 is on a straight-ish stretch; the blocker is parked 6 rows ahead.
    let me = car(&t, 7, 20, 8, 12.0, 0, false);
    let blocker = car(&t, 8, 26, 8, 0.0, 0, false);
    let mut f = field(&t, Skill::Advanced, false, &[7]);
    let mut lanes = vec![];
    for _ in 0..80 {
        f.control(&[me, blocker]);
        lanes.push(f.decision(7).unwrap().lane);
    }
    assert!(
        lanes.iter().any(|l| *l != 8),
        "never left the blocked lane: {lanes:?}"
    );
    // One lane per decision at most.
    for w in lanes.windows(2) {
        assert!((w[1] - w[0]).abs() <= 1, "{lanes:?}");
    }
    // The chosen lane stays on the road (driving lanes 5..=8, shoulders 4 and 9).
    assert!(lanes.iter().all(|l| (4..=9).contains(l)), "{lanes:?}");
    // The free-road target is not collapsed to the blocker's speed once the
    // lane is clear of it.
    let decision = f.decision(7).unwrap();
    assert!(decision.free_target_speed > 3.0);
}

#[test]
fn a_car_beside_in_the_neighbouring_lane_is_not_cut_into() {
    let t = track();
    // Car in lane 7 with another alongside in lane 8 and a blocker ahead in 7:
    // the free lane 6 is chosen, not the occupied lane 8.
    let me = car(&t, 7, 20, 7, 12.0, 0, false);
    let beside = car(&t, 8, 20, 8, 12.0, 0, false);
    let blocker = car(&t, 9, 25, 7, 0.0, 0, false);
    let mut f = field(&t, Skill::Advanced, false, &[7]);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..40 {
        f.control(&[me, beside, blocker]);
        seen.insert(f.decision(7).unwrap().lane);
    }
    assert!(!seen.contains(&8), "moved into the occupied lane: {seen:?}");
    assert!(seen.contains(&6), "did not use the free lane: {seen:?}");
}

#[test]
fn catch_up_raises_pace_behind_and_lowers_it_ahead_of_the_leading_human() {
    let t = track();
    let rows = t.len();
    assert!(rows > 45);
    let pace = |me_row: usize, lead_row: usize, catch_up: bool| {
        let me = car(&t, 7, me_row, 8, 10.0, 1, false);
        let leader = car(&t, 1, lead_row, 8, 10.0, 1, true);
        let mut f = field(&t, Skill::Advanced, catch_up, &[7]);
        f.control(&[leader, me]);
        f.decision(7).unwrap().pace
    };
    let behind = pace(15, 40, true);
    let ahead = pace(40, 15, true);
    let level = pace(28, 28, true);
    assert!(behind > level && level > ahead, "{behind} {level} {ahead}");
    // Switching catch-up off removes the effect.
    let off_behind = pace(15, 40, false);
    let off_ahead = pace(40, 15, false);
    assert!(
        (off_behind - off_ahead).abs() < 1e-9,
        "{off_behind} {off_ahead}"
    );
}

#[test]
fn beginners_aim_lower_than_advanced_drivers() {
    let t = track();
    let target = |skill| {
        let me = car(&t, 7, 10, 8, 10.0, 1, false);
        let mut f = field(&t, skill, false, &[7]);
        f.control(&[me]);
        f.decision(7).unwrap().free_target_speed
    };
    assert!(target(Skill::Beginner) < target(Skill::Advanced));
}

#[test]
fn finished_cars_receive_no_control_and_humans_are_never_controlled() {
    let t = track();
    let mut done = car(&t, 7, 10, 8, 10.0, 2, false);
    done.finished = true;
    let human = car(&t, 1, 20, 7, 5.0, 0, true);
    let mut f = field(&t, Skill::Advanced, true, &[7]);
    let controls = f.control(&[done, human]);
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].0, 7);
    assert_eq!(controls[0].1, racer_sim::ScriptedControl::default());
}

#[test]
fn opponents_use_distinct_personalities_in_order() {
    let t = track();
    let cars: Vec<CarObs> = (0..3)
        .map(|k| car(&t, 10 + k, 10 + 3 * k as usize, 8, 10.0, 1, false))
        .collect();
    let mut f = field(&t, Skill::Advanced, false, &[10, 11, 12]);
    f.control(&cars);
    let targets: Vec<f64> = (10..13)
        .map(|id| f.decision(id).unwrap().free_target_speed)
        .collect();
    // Different tiers (and rows) give different pace targets.
    assert!(
        targets.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-6),
        "{targets:?}"
    );
}
