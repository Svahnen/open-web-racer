//! Race course (gates, laps, finish) and computer-driver road for an imported
//! track, derived from its road-edge rows alone.
//!
//! The rows run along the road and give a centre position, a forward vector
//! and the distances to both road edges. A track whose rows come back to where
//! they started is a circuit (laps); any other track is point to point and
//! finishes at its last row. The player's start pose decides the driving
//! direction, so a track whose rows run against the start heading is driven
//! the other way.

use racer_sim::{Checkpoint, TrackEdgeRow, TrackRuntime, Vec2};

use crate::race::road_frames_from_edge_rows;
use crate::track::{AiTrack, DeriveParams, RoadFrame};

/// Fewest road rows a course is built from.
const MIN_ROWS: usize = 16;
/// Shortest route (metres) that makes a race.
const MIN_LENGTH: f64 = 400.0;
/// Distance between consecutive gates (metres).
const GATE_SPACING: f64 = 90.0;
/// How far the finish line sits ahead of the rearmost grid slot (metres): the
/// whole grid (eight cars, eight metres apart) starts behind it.
const LINE_AHEAD: f64 = 80.0;
/// Rows further apart than this are not one road.
const MAX_ROW_GAP: f64 = 120.0;
/// A route whose end is this close (or closer) to its start is a circuit,
/// at least this many mean row spacings.
const CLOSE_GAP: f64 = 30.0;

/// Gates and road of an imported track, for one driving direction.
#[derive(Clone, Debug)]
pub struct ImportedCourse {
    /// Gates (gate 0 is the finish line); `laps_to_finish` is 1, see
    /// [`Self::runtime_for_laps`].
    pub runtime: TrackRuntime,
    /// Lanes and speed profile of the road, in driving direction, with the
    /// start grid placed so the rearmost slot is at the start pose.
    pub ai_track: AiTrack,
    /// True for a circuit (laps), false for point to point (one run).
    pub closed: bool,
    /// Length of one lap, or of the run, in metres.
    pub length: f64,
}

impl ImportedCourse {
    /// The gate runtime for `laps` laps (a point-to-point course always has
    /// one run; `laps` is clamped to at least one).
    pub fn runtime_for_laps(&self, laps: u32) -> TrackRuntime {
        let mut runtime = self.runtime.clone();
        runtime.laps_to_finish = if self.closed { laps.max(1) } else { 1 };
        runtime
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Build the course of an imported track. `spawn` and `heading` are the
/// player's start pose in the same coordinates as the rows. Returns `None`
/// when the rows do not describe a road long enough to race on.
pub fn imported_course(rows: &[TrackEdgeRow], spawn: Vec2, heading: f64) -> Option<ImportedCourse> {
    if rows.len() < MIN_ROWS {
        return None;
    }
    let mut frames = road_frames_from_edge_rows(rows, 1.0);
    if frames.iter().any(|f| {
        !f.position.iter().all(|v| v.is_finite()) || !(f.left_dist + f.right_dist > 0.5)
    }) {
        return None;
    }
    let mean_gap = frames
        .windows(2)
        .map(|w| dist(w[0].position, w[1].position))
        .sum::<f64>()
        / (frames.len() - 1) as f64;
    // Rows can repeat a position (zero gap); only large jumps break the road.
    if frames
        .windows(2)
        .any(|w| dist(w[0].position, w[1].position) > MAX_ROW_GAP)
    {
        return None;
    }
    let closed = dist(frames[0].position, frames[frames.len() - 1].position)
        < CLOSE_GAP.max(3.0 * mean_gap);

    // Drive in the direction the player faces at the start.
    let nearest = |frames: &[RoadFrame]| {
        (0..frames.len())
            .min_by(|a, b| {
                let d = |i: usize| dist(frames[i].position, [spawn.x, spawn.y]);
                d(*a).total_cmp(&d(*b))
            })
            .unwrap_or(0)
    };
    let near0 = nearest(&frames);
    let f = frames[near0].forward;
    if f[0] * heading.cos() + f[1] * heading.sin() < 0.0 {
        frames.reverse();
        for frame in &mut frames {
            frame.forward = [-frame.forward[0], -frame.forward[1]];
            std::mem::swap(&mut frame.left_dist, &mut frame.right_dist);
        }
    }
    let near = nearest(&frames);

    let mut road = AiTrack::from_frames(&frames, closed, &DeriveParams::default()).ok()?;
    road.grid_spacing = (8.0 / road.row_length).round().max(1.0) as usize;
    road.grid_start = road.step(near, -1, 1);

    // Cumulative distance from the start row, along the driving direction.
    let n = frames.len();
    let seg = |i: usize| -> f64 {
        let j = (i + 1) % n;
        dist(frames[i].position, frames[j].position)
    };
    let length = if closed {
        (0..n).map(seg).sum::<f64>()
    } else {
        (near..n - 1).map(seg).sum::<f64>()
    };
    if length < MIN_LENGTH {
        return None;
    }
    // Position and half width at a distance `d` ahead of the start row.
    let at = |d: f64| -> (Vec2, f64) {
        let mut row = near;
        let mut left = d;
        loop {
            let next = if closed {
                (row + 1) % n
            } else {
                (row + 1).min(n - 1)
            };
            let step = dist(frames[row].position, frames[next].position);
            if left <= step || next == row {
                let t = if step > 1e-9 { (left / step).clamp(0.0, 1.0) } else { 0.0 };
                let (a, b) = (&frames[row], &frames[next]);
                let p = Vec2::new(
                    a.position[0] + (b.position[0] - a.position[0]) * t,
                    a.position[1] + (b.position[1] - a.position[1]) * t,
                );
                let half = a.left_dist.max(a.right_dist);
                return (p, half);
            }
            left -= step;
            row = next;
        }
    };
    let gate = |d: f64| -> Checkpoint {
        let (center, half) = at(d);
        Checkpoint {
            center,
            radius: (half + 4.0).clamp(9.0, 24.0),
        }
    };

    let mut checkpoints = Vec::new();
    if closed {
        let count = ((length / GATE_SPACING).round() as usize).clamp(3, 400);
        let step = length / count as f64;
        for k in 0..count {
            checkpoints.push(gate((LINE_AHEAD + k as f64 * step) % length));
        }
    } else {
        // Finish at the last row (slightly short of it); gates between.
        let run = (length - 6.0).max(MIN_LENGTH - 6.0);
        let count = ((run / GATE_SPACING).round() as usize).clamp(3, 400);
        checkpoints.push(gate(run));
        for k in 1..count {
            checkpoints.push(gate(k as f64 * run / count as f64));
        }
    }
    let runtime = TrackRuntime {
        min: Vec2::new(-f64::MAX, -f64::MAX),
        max: Vec2::new(f64::MAX, f64::MAX),
        checkpoints,
        laps_to_finish: 1,
    };
    Some(ImportedCourse {
        runtime,
        ai_track: road,
        closed,
        length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(x: f64, y: f64, fx: f64, fy: f64) -> TrackEdgeRow {
        TrackEdgeRow {
            x,
            y,
            z: 0.0,
            forward: Vec2::new(fx, fy),
            right: Vec2::new(fy, -fx),
            left_distance: 7.0,
            right_distance: 7.0,
            block: 0,
        }
    }

    /// A straight road along +X, 4 m per row.
    fn straight(rows: usize) -> Vec<TrackEdgeRow> {
        (0..rows).map(|i| row(i as f64 * 4.0 - 40.0, 0.0, 1.0, 0.0)).collect()
    }

    /// A stadium-shaped loop (two straights, two half circles), anticlockwise,
    /// starting at the middle of the bottom straight.
    fn stadium() -> Vec<TrackEdgeRow> {
        let (half, r) = (150.0_f64, 60.0_f64);
        let mut pts = Vec::new();
        let mut x = 0.0;
        while x < half {
            pts.push(row(x, -r, 1.0, 0.0));
            x += 4.0;
        }
        let arc = std::f64::consts::PI * r;
        let mut s = 0.0;
        while s < arc {
            let a = -std::f64::consts::FRAC_PI_2 + s / r;
            pts.push(row(half + r * a.cos(), r * a.sin(), -a.sin(), a.cos()));
            s += 4.0;
        }
        let mut x = half;
        while x > -half {
            pts.push(row(x, r, -1.0, 0.0));
            x -= 4.0;
        }
        let mut s = 0.0;
        while s < arc {
            let a = std::f64::consts::FRAC_PI_2 + s / r;
            pts.push(row(-half + r * a.cos(), r * a.sin(), -a.sin(), a.cos()));
            s += 4.0;
        }
        let mut x = -half;
        while x < 0.0 {
            pts.push(row(x, -r, 1.0, 0.0));
            x += 4.0;
        }
        pts
    }

    #[test]
    fn too_short_or_degenerate_rows_make_no_course() {
        assert!(imported_course(&straight(10), Vec2::new(0.0, 0.0), 0.0).is_none());
        // 30 rows of 4 m: 116 m, below the minimum race length.
        assert!(imported_course(&straight(30), Vec2::new(0.0, 0.0), 0.0).is_none());
    }

    #[test]
    fn straight_road_is_a_point_to_point_run_that_finishes_at_the_end() {
        let rows = straight(400);
        let course = imported_course(&rows, Vec2::new(0.0, 0.0), 0.0).expect("course");
        assert!(!course.closed);
        assert_eq!(course.runtime_for_laps(3).laps_to_finish, 1, "p2p ignores laps");
        let finish = course.runtime.checkpoints[0];
        assert!(finish.center.x > 1400.0, "finish near the end: {finish:?}");
        // Gates ascend along the road.
        let xs: Vec<f64> = course.runtime.checkpoints.iter().skip(1).map(|g| g.center.x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        // Driving the gates in order finishes the run.
        let mut world = racer_sim::RaceWorld::new(racer_sim::SimTiming::default(), course.runtime_for_laps(1));
        world.place_vehicle(Vec2::new(0.0, 0.0), 0.0);
        for g in course.runtime.checkpoints.iter().skip(1).chain(std::iter::once(&finish)) {
            // Teleport-free pass: drive the vehicle across the gate in one step.
            world.vehicle.position = Vec2::new(g.center.x - 1.0, g.center.y);
            world.vehicle.velocity = Vec2::new(40.0, 0.0);
            world.step(racer_sim::DriverInput::default());
        }
        assert!(world.vehicle.finished, "cp {} laps {}", world.vehicle.checkpoint, world.vehicle.completed_laps);
    }

    #[test]
    fn rows_against_the_start_heading_are_driven_the_other_way() {
        let rows = straight(400);
        // Spawn near the far end facing -X.
        let spawn = Vec2::new(1400.0, 0.0);
        let course = imported_course(&rows, spawn, std::f64::consts::PI).expect("course");
        let finish = course.runtime.checkpoints[0];
        assert!(finish.center.x < 0.0, "finish is at the low-x end: {finish:?}");
        assert!(course.ai_track.rows[0].forward[0] < 0.0);
    }

    #[test]
    fn loop_is_a_circuit_with_the_finish_line_ahead_of_the_grid() {
        let rows = stadium();
        let course = imported_course(&rows, Vec2::new(0.0, -60.0), 0.0).expect("course");
        assert!(course.closed);
        assert_eq!(course.runtime_for_laps(3).laps_to_finish, 3);
        assert!(course.length > 900.0 && course.length < 1050.0, "{}", course.length);
        let line = course.runtime.checkpoints[0];
        assert!((line.center.x - 80.0).abs() < 2.0 && (line.center.y + 60.0).abs() < 2.0, "{line:?}");
        // The grid's rearmost slot is at the start; the front slots are short of the line.
        let (p0, _) = course.ai_track.grid_slot(0, 8, 1);
        let (p7, _) = course.ai_track.grid_slot(7, 8, 1);
        assert!(p0[0].abs() < 8.0, "{p0:?}");
        assert!(p7[0] > p0[0] + 40.0 && p7[0] < 80.0, "{p7:?}");
    }

    #[test]
    fn opponents_race_a_circuit_and_a_run_to_their_finish() {
        use crate::{AiConfig, Opponents, Settings};
        use std::sync::Arc;
        for (rows, laps, spawn, heading) in [
            (stadium(), 2u32, Vec2::new(0.0, -60.0), 0.0),
            (straight(500), 1u32, Vec2::new(0.0, 0.0), 0.0),
        ] {
            let course = imported_course(&rows, spawn, heading).expect("course");
            let runtime = course.runtime_for_laps(laps);
            let mut opp = Opponents::new(
                &runtime,
                Arc::new(course.ai_track.clone()),
                Arc::new(AiConfig::default()),
                Settings {
                    laps: runtime.laps_to_finish,
                    ..Settings::default()
                },
                4,
                1000,
                1,
                racer_sim::SimTiming::default(),
            );
            for _ in 0..(60 * 240) {
                opp.step(&[]);
                if opp.cars.iter().all(|c| c.world.vehicle.finished) {
                    break;
                }
            }
            assert!(
                opp.cars.iter().all(|c| c.world.vehicle.finished),
                "closed={} {:?}",
                course.closed,
                opp.cars
                    .iter()
                    .map(|c| (c.world.vehicle.completed_laps, c.world.vehicle.checkpoint, c.world.vehicle.position))
                    .collect::<Vec<_>>()
            );
        }
    }
}
