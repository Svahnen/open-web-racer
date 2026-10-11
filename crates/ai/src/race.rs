//! Bridge to the simulation: builds a road from a gate circuit, creates the
//! opponents' worlds on the grid and advances them one fixed tick at a time.
//! Contact between cars and the room's other rules stay with the caller.

use std::sync::Arc;

use racer_sim::{DriverInput, RaceWorld, SimTiming, TrackRuntime, Vec2, VehicleModel};

use crate::config::AiConfig;
use crate::field::{AiField, CarObs, Settings};
use crate::track::{AiTrack, DeriveParams, TrackError, smooth_loop};

/// Contact radius of a computer-driven project car (metres).
pub const OPPONENT_CONTACT_RADIUS: f64 = 1.0;
/// Half width used for lane geometry (metres).
pub const OPPONENT_HALF_WIDTH: f64 = 1.0;
/// Most opponents a race can hold (the shared contact pass handles eight cars).
pub const MAX_OPPONENTS: usize = 7;
/// Default number of opponents.
pub const DEFAULT_OPPONENTS: usize = 7;

/// Lane data and speed profile for the project test circuit, derived from the
/// gate loop: a smooth closed centre line, 3 m lanes, two per side, corner
/// speeds from curvature.
pub fn circuit_ai_track(track: &TrackRuntime) -> Result<AiTrack, TrackError> {
    let gates: Vec<[f64; 2]> = track
        .checkpoints
        .iter()
        .map(|g| [g.center.x, g.center.y])
        .collect();
    let points = smooth_loop(&gates, 2.0);
    let mut ai = AiTrack::from_centerline(
        &points,
        true,
        6.0,
        6.0,
        &DeriveParams {
            max_speed: 30.0,
            ..DeriveParams::default()
        },
    )?;
    ai.grid_start = 0;
    ai.grid_spacing = 2;
    Ok(ai)
}

/// One computer-controlled car and its world.
#[derive(Clone, Debug)]
pub struct Opponent {
    pub id: u32,
    pub world: RaceWorld,
}

/// The opponents of a race.
#[derive(Clone, Debug)]
pub struct Opponents {
    pub field: AiField,
    pub cars: Vec<Opponent>,
}

impl Opponents {
    /// Create `count` opponents with ids `first_id..`. Grid positions
    /// `0..humans` belong to the human cars (see [`Self::human_grid_slot`]);
    /// opponent `k` starts at position `humans + k`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: &TrackRuntime,
        ai_track: Arc<AiTrack>,
        config: Arc<AiConfig>,
        settings: Settings,
        count: usize,
        first_id: u32,
        humans: usize,
        timing: SimTiming,
    ) -> Self {
        let count = count.min(MAX_OPPONENTS);
        let ids: Vec<u32> = (0..count as u32).map(|k| first_id + k).collect();
        let field = AiField::new(ai_track, config, settings, &ids);
        let total = humans + count;
        let cars = ids
            .iter()
            .enumerate()
            .map(|(k, id)| {
                let mut world = RaceWorld::new(timing, runtime.clone());
                let _ = world.set_vehicle_model(VehicleModel::Scripted);
                let (position, heading) = field.grid_slot(humans + k, total);
                world.place_vehicle(Vec2::new(position[0], position[1]), heading);
                Opponent { id: *id, world }
            })
            .collect();
        Self { field, cars }
    }

    /// Give opponent `id` the limits of its own car (see
    /// [`crate::vehicle_ai_from_arcade`]).
    pub fn set_vehicle(&mut self, id: u32, vehicle: racer_pack_schema::ai::VehicleAi) {
        self.field.set_vehicle(id, vehicle);
    }

    /// Give every opponent the ground, wall and road-edge collision of a
    /// track with relief (generated tracks); without it they drive on a flat
    /// plane. Returns how many accepted the geometry.
    pub fn install_collision(&mut self, collision: &racer_sim::PreviewCollisionWorld) -> usize {
        self.cars
            .iter_mut()
            .filter_map(|c| c.world.install_preview_collision(collision.clone()).ok())
            .count()
    }

    /// World pose of a human's grid position among `humans + count` cars.
    pub fn human_grid_slot(&self, index: usize, humans: usize) -> (Vec2, f64) {
        let total = humans + self.cars.len();
        let (p, h) = self.field.grid_slot(index, total);
        (Vec2::new(p[0], p[1]), h)
    }

    /// Advance every opponent one fixed tick. `humans` are the other cars the
    /// drivers react to; opponent observations are added automatically.
    pub fn step(&mut self, humans: &[CarObs]) {
        let mut observed: Vec<CarObs> = humans.to_vec();
        observed.extend(
            self.cars
                .iter()
                .map(|c| CarObs::from_world(c.id, &c.world, false, OPPONENT_HALF_WIDTH)),
        );
        for (id, control) in self.field.control(&observed) {
            if let Some(car) = self.cars.iter_mut().find(|c| c.id == id) {
                car.world.set_scripted_control(control);
            }
        }
        for car in &mut self.cars {
            car.world.step(DriverInput::default());
        }
    }
}

/// Road frames from imported road-edge rows (hook for imported tracks). The
/// rows give a position, a forward and right vector and the distances to the
/// road edges; `metres_per_unit` converts the row units to metres. Lane layouts
/// come from the widths (`AiTrack::from_frames`); explicit lane and speed
/// tables, when a content importer provides them, are applied afterwards with
/// `AiTrack::set_row_lanes` and `AiTrack::set_speed_profile`.
pub fn road_frames_from_edge_rows(
    rows: &[racer_sim::TrackEdgeRow],
    metres_per_unit: f64,
) -> Vec<crate::track::RoadFrame> {
    rows.iter()
        .map(|r| crate::track::RoadFrame {
            position: [r.x * metres_per_unit, r.y * metres_per_unit],
            forward: [r.forward.x, r.forward.y],
            left_dist: r.left_distance * metres_per_unit,
            right_dist: r.right_distance * metres_per_unit,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use racer_sim::TrackEdgeRow;

    #[test]
    fn imported_edge_rows_become_a_road_with_lanes_and_speeds() {
        let rows: Vec<TrackEdgeRow> = (0..40)
            .map(|i| TrackEdgeRow {
                x: f64::from(i) * 4.0,
                y: 0.0,
                z: 0.0,
                forward: Vec2::new(1.0, 0.0),
                right: Vec2::new(0.0, -1.0),
                left_distance: 7.0,
                right_distance: 7.0,
                block: 0,
            })
            .collect();
        let frames = road_frames_from_edge_rows(&rows, 1.0);
        let track = AiTrack::from_frames(&frames, false, &DeriveParams::default()).unwrap();
        assert_eq!(track.len(), 40);
        assert_eq!(
            (track.rows[3].left_count, track.rows[3].right_count),
            (2, 2)
        );
        // A straight road gets the top speed everywhere.
        assert!(track.profile_forward.iter().all(|v| *v >= 30));
        // The hook accepts imported tables.
        let mut track = track;
        track.set_speed_profile(2, vec![21; 20], Vec::new());
        assert_eq!(track.section_speed(10, 1), 21);
    }
}
