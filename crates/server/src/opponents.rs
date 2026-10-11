//! Computer opponents of a room: creation on the grid when the countdown
//! starts, per-tick driving, and their snapshot entries.
//!
//! Opponents are not players: they have no connection, input or readiness and
//! are not part of the internal rollback frame. They use the same fixed step,
//! contact pass, ranking and snapshot as human cars.

use std::sync::Arc;

use racer_ai::config::Skill;
use racer_ai::field::{CarObs, Settings};
use racer_ai::race::{OPPONENT_CONTACT_RADIUS, OPPONENT_HALF_WIDTH};
use racer_ai::{AiConfig, Opponents, circuit_ai_track, vehicle_ai_from_classic};
use racer_content::classic_params::project_classic_config;
use racer_content::project_cars::project_cars;
use racer_protocol::{HANDLING_SCRIPTED, MAX_PLAYERS_PER_ROOM, VehicleSnapshot};
use racer_sim::{RaceWorld, SimTiming, Vec2};

use super::{GameplayTrack, Room, new_track_runtime};

/// Opponent ids start here; connection ids stay far below.
pub(crate) const AI_ID_BASE: u32 = 1_000_000_000;

/// Paint colours handed to opponents in order (valid `paint-colour-N` ids).
const OPPONENT_PAINTS: [u8; 7] = [2, 6, 4, 5, 1, 3, 7];

#[derive(Clone)]
pub(crate) struct RoomAi {
    pub opponents: Opponents,
    pub vehicle_profile_id: String,
    pub vehicle_fingerprint: String,
    /// Generic car model and paint scheme of each opponent, in grid order.
    pub car_picks: Vec<(String, u8)>,
    /// Imported car (profile id, fingerprint, paint id) of each opponent on
    /// an imported track, in grid order; empty elsewhere.
    pub owner_cars: Vec<(String, String, String)>,
}

/// Number of opponents an imported-track room starts with.
pub(crate) fn owner_ai_count(room: &Room) -> usize {
    usize::from(room.summary.ai_count)
        .min(MAX_PLAYERS_PER_ROOM.saturating_sub(room.players.len()))
}

/// The imported car of each of `count` opponents on `track`: the track's cars
/// in profile-id order, starting with the one after the host's, so the field
/// is varied and every client can work out which models it needs in the lobby
/// (the browser mirrors this rule). Every opponent wears the host's paint, so
/// a client prepares at most one extra model variant per car of the track.
pub(crate) fn owner_ai_roster(
    track: &GameplayTrack,
    host_profile: &str,
    host_paint: &str,
    count: usize,
) -> Vec<(String, String, String)> {
    let mut ids: Vec<&String> = track.cars.keys().collect();
    ids.sort();
    if ids.is_empty() {
        return Vec::new();
    }
    let host = ids.iter().position(|id| id.as_str() == host_profile).unwrap_or(0);
    (0..count)
        .filter_map(|k| {
            let index = (host + 1 + k) % ids.len();
            let car = track.cars.get(ids[index])?;
            Some((
                car.profile_id.clone(),
                car.source_fingerprint.clone(),
                host_paint.to_owned(),
            ))
        })
        .collect()
}

pub(crate) fn is_ai_id(id: u32) -> bool {
    id >= AI_ID_BASE
}

/// Create the opponents and line everybody up on the grid. Rooms without
/// opponents (and open-drive rooms) keep their classic spawn.
pub(crate) fn prepare_start(room: &mut Room) {
    room.ai = None;
    let humans = room.players.len();
    if humans == 0 {
        return;
    }
    if let Some(course) = room.owner_course() {
        prepare_owner_start(room, &course);
        return;
    }
    if room.summary.open_drive || room.summary.ai_count == 0 {
        return;
    }
    let count = usize::from(room.summary.ai_count).min(MAX_PLAYERS_PER_ROOM.saturating_sub(humans));
    if count == 0 {
        return;
    }
    let project = room.gameplay_track.as_ref().and_then(|t| {
        let p = t.project.as_ref()?;
        let road = p.ai_track.clone()?;
        let car = t.cars.get("vehicle-profile-project-default")?;
        Some((p.runtime.clone(), road, car.collision.clone()))
    });
    let (runtime, track, collision) = match project {
        Some((mut runtime, road, collision)) => {
            runtime.laps_to_finish = u32::from(room.summary.laps);
            (runtime, road.as_ref().clone(), Some(collision))
        }
        None if room
            .gameplay_track
            .as_ref()
            .is_some_and(|t| t.project.is_some()) =>
        {
            return;
        }
        None => {
            let runtime = new_track_runtime(room.summary.laps, false);
            let Ok(track) = circuit_ai_track(&runtime) else {
                return;
            };
            (runtime, track, None)
        }
    };
    let settings = Settings {
        skill: Skill::from_level(room.summary.ai_skill),
        catch_up: room.summary.ai_catch_up,
        laps: u32::from(room.summary.laps),
        seed: room.summary.race_id.wrapping_mul(7919).wrapping_add(1),
        ..Settings::default()
    };
    let mut opponents = Opponents::new(
        &runtime,
        Arc::new(track),
        Arc::new(AiConfig::default()),
        settings,
        count,
        AI_ID_BASE,
        humans,
        SimTiming::default(),
    );
    if let Some(collision) = &collision {
        opponents.install_collision(collision);
    }
    let mut ids: Vec<u32> = room.players.keys().copied().collect();
    ids.sort_unstable();
    for (index, id) in ids.iter().enumerate() {
        let (position, heading) = opponents.human_grid_slot(index, humans);
        if let Some(player) = room.players.get_mut(id) {
            player.world.place_vehicle(position, heading);
        }
    }
    let host = room
        .players
        .get(&room.owner_player_id)
        .or_else(|| room.players.values().next());
    // Opponent cars follow from the host's pick (or the first model), so every
    // client that receives the snapshots shows the same field.
    let cars = project_cars();
    let base = host
        .and_then(|p| {
            let car = cars.index_of(&p.car_model)?;
            Some((car, usize::from(p.car_variant)))
        })
        .unwrap_or((0, 0));
    let car_picks: Vec<(String, u8)> = (0..count)
        .map(|k| {
            let (car, paint) = cars.opponent_pick(base, k);
            (cars.cars()[car].id.clone(), paint as u8)
        })
        .collect();
    // Each opponent respects the limits of the car it was handed.
    for (k, (car_id, _)) in car_picks.iter().enumerate() {
        if let Some(config) = project_classic_config(car_id) {
            opponents.set_vehicle(AI_ID_BASE + k as u32, vehicle_ai_from_classic(&config));
        }
    }
    room.ai = Some(RoomAi {
        car_picks,
        owner_cars: Vec::new(),
        opponents,
        vehicle_profile_id: host.map_or_else(
            || "vehicle-profile-project-default".to_string(),
            |p| p.vehicle_profile_id.clone(),
        ),
        vehicle_fingerprint: host.map_or_else(|| "0".repeat(64), |p| p.vehicle_fingerprint.clone()),
    });
}

/// Start of a race on an imported track: everybody lines up on the course's
/// grid (the humans take the rearmost slots in id order) and the opponents,
/// if any, are created with imported cars and the host car's ground contact.
fn prepare_owner_start(room: &mut Room, course: &racer_ai::ImportedCourse) {
    let humans = room.players.len();
    let count = owner_ai_count(room);
    let total = humans + count;
    let mut ids: Vec<u32> = room.players.keys().copied().collect();
    ids.sort_unstable();
    for (index, id) in ids.iter().enumerate() {
        let (position, heading) = course.ai_track.grid_slot(index, total, 1);
        if let Some(player) = room.players.get_mut(id) {
            player
                .world
                .place_vehicle(Vec2::new(position[0], position[1]), heading);
        }
    }
    if count == 0 {
        return;
    }
    let Some(track) = room.gameplay_track.clone() else {
        return;
    };
    let Some(host) = room
        .players
        .get(&room.owner_player_id)
        .or_else(|| room.players.values().next())
    else {
        return;
    };
    let host_profile = host.vehicle_profile_id.clone();
    let host_paint = host.paint_id.clone();
    let vehicle_fingerprint = host.vehicle_fingerprint.clone();
    let Some(collision) = track.cars.get(&host_profile).map(|c| c.collision.clone()) else {
        return;
    };
    let runtime = course.runtime_for_laps(u32::from(room.summary.laps));
    let settings = Settings {
        skill: Skill::from_level(room.summary.ai_skill),
        catch_up: room.summary.ai_catch_up,
        laps: runtime.laps_to_finish,
        seed: room.summary.race_id.wrapping_mul(7919).wrapping_add(1),
        ..Settings::default()
    };
    let mut opponents = Opponents::new(
        &runtime,
        Arc::new(course.ai_track.clone()),
        Arc::new(AiConfig::default()),
        settings,
        count,
        AI_ID_BASE,
        humans,
        SimTiming::default(),
    );
    opponents.install_collision(&collision);
    room.ai = Some(RoomAi {
        owner_cars: owner_ai_roster(&track, &host_profile, &host_paint, count),
        car_picks: Vec::new(),
        opponents,
        vehicle_profile_id: host_profile,
        vehicle_fingerprint,
    });
}

/// Drive the opponents one fixed tick (before the contact pass). Returns each
/// opponent's pre-step position for the contact sweep.
pub(crate) fn step(room: &mut Room) -> Vec<(u32, Vec2)> {
    let Some(ai) = room.ai.as_mut() else {
        return Vec::new();
    };
    let mut humans: Vec<(u32, CarObs)> = room
        .players
        .iter()
        .map(|(id, p)| {
            (
                *id,
                CarObs::from_world(*id, &p.world, true, OPPONENT_HALF_WIDTH),
            )
        })
        .collect();
    humans.sort_by_key(|(id, _)| *id);
    let humans: Vec<CarObs> = humans.into_iter().map(|(_, o)| o).collect();
    let before: Vec<(u32, Vec2)> = ai
        .opponents
        .cars
        .iter()
        .map(|c| (c.id, c.world.vehicle.position))
        .collect();
    ai.opponents.step(&humans);
    before
}

/// Contact parameters shared by every opponent.
pub(crate) fn contact_profile() -> (f64, f64, f64) {
    (OPPONENT_CONTACT_RADIUS, -0.5, 0.5)
}

/// Snapshot rows of the opponents. `position_of` maps an id to its race rank.
pub(crate) fn snapshots(
    room: &Room,
    position_of: &dyn Fn(u32) -> u8,
    ticks32: &dyn Fn(u64) -> u32,
) -> Vec<VehicleSnapshot> {
    let Some(ai) = &room.ai else {
        return Vec::new();
    };
    ai.opponents
        .cars
        .iter()
        .enumerate()
        .map(|(k, car)| {
            let w = &car.world;
            let v = w.vehicle;
            let laps = w.lap_times();
            let owner_car = ai.owner_cars.get(k);
            VehicleSnapshot {
                player_id: car.id,
                driver_name: format!("Computer {}", k + 1),
                vehicle_profile_id: owner_car
                    .map_or_else(|| ai.vehicle_profile_id.clone(), |c| c.0.clone()),
                vehicle_fingerprint: owner_car
                    .map_or_else(|| ai.vehicle_fingerprint.clone(), |c| c.1.clone()),
                paint_id: owner_car.map_or_else(
                    || {
                        format!(
                            "paint-colour-{}",
                            OPPONENT_PAINTS[k % OPPONENT_PAINTS.len()]
                        )
                    },
                    |c| c.2.clone(),
                ),
                car_model: ai.car_picks.get(k).map(|c| c.0.clone()).unwrap_or_default(),
                car_variant: ai.car_picks.get(k).map_or(0, |c| c.1),
                x: v.position.x,
                y: v.position.y,
                z: v.position_z,
                velocity_x: v.velocity.x,
                velocity_y: v.velocity.y,
                heading: v.heading,
                speed: v.speed(),
                ground_normal: [0.0, 0.0, 1.0],
                checkpoint: v.checkpoint as u16,
                completed_laps: v.completed_laps,
                finished: v.finished,
                race_position: position_of(car.id),
                current_lap_ticks: ticks32(w.current_lap_ticks()),
                last_lap_ticks: laps.last_lap_ticks.map(ticks32),
                best_lap_ticks: laps.best_lap_ticks.map(ticks32),
                finish_ticks: laps.finish_ticks.map(ticks32),
                gear: 0,
                // Engine-speed proxy for remote voices (opponents have no gearbox).
                rpm_q8: (60.0 + v.speed() * 7.0).clamp(0.0, 255.0) as u8,
                handling_model: HANDLING_SCRIPTED,
                is_ai: true,
            }
        })
        .collect()
}

/// Worlds of the opponents with their ids (for ranking).
pub(crate) fn worlds(room: &Room) -> Vec<(u32, &RaceWorld)> {
    room.ai
        .as_ref()
        .map(|ai| ai.opponents.cars.iter().map(|c| (c.id, &c.world)).collect())
        .unwrap_or_default()
}
