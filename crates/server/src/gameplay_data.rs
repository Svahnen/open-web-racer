//! Gameplay data catalogue for imported open-drive profiles.
//! Source bytes stay in this process and are never served or placed on the wire.

use racer_sim::PreviewCollisionWorld;
use std::collections::HashMap;

/// Profile ids of the default synthetic acceptance-test catalogue.
#[cfg(any(test, feature = "synthetic-multiplayer-test"))]
const SYNTHETIC_TRACK_IDS: [&str; 8] = [
    "track-profile-0001",
    "track-profile-0002",
    "track-profile-0003",
    "track-profile-0004",
    "track-profile-0005",
    "track-profile-0006",
    "track-profile-0007",
    "track-profile-0008",
];
#[cfg(any(test, feature = "synthetic-multiplayer-test"))]
const SYNTHETIC_CAR_IDS: [&str; 2] = ["vehicle-profile-0001", "vehicle-profile-0002"];

#[derive(Clone)]
pub struct GameplayDataStore {
    tracks: HashMap<String, std::sync::Arc<GameplayTrack>>,
    diagnostics: Vec<String>,
}

#[derive(Clone)]
pub struct GameplayTrack {
    pub profile_id: String,
    pub source_fingerprint: String,
    pub cars: HashMap<String, GameplayCar>,
    /// Lap race data of a generated project track (None for imported tracks).
    pub project: Option<ProjectRuntime>,
    /// Gates, laps and computer-driver road of an imported track, derived from
    /// its road rows (None when the rows do not describe a raceable road; the
    /// track then only supports free driving). Filled by
    /// [`GameplayDataStore::from_parts`].
    pub course: Option<std::sync::Arc<racer_ai::ImportedCourse>>,
}

impl GameplayTrack {
    /// Derive [`Self::course`] from the road rows of the track's cars (all
    /// cars of a track share one road).
    pub fn derive_course(&mut self) {
        if self.project.is_some() || self.course.is_some() {
            return;
        }
        let mut ids: Vec<&String> = self.cars.keys().collect();
        ids.sort();
        self.course = ids
            .into_iter()
            .filter_map(|id| self.cars.get(id))
            .find_map(|car| {
                racer_ai::imported_course(
                    &car.collision.edge_rows,
                    racer_sim::Vec2::new(0.0, 0.0),
                    0.0,
                )
            })
            .map(std::sync::Arc::new);
    }
}

/// Gates, laps and start grid of a generated project track, in simulation
/// coordinates (grid slot 0 is the origin).
#[derive(Clone)]
pub struct ProjectRuntime {
    pub runtime: racer_sim::TrackRuntime,
    pub spawn_slots: Vec<racer_sim::Vec2>,
    /// Road, lanes and speed profile for computer opponents.
    pub ai_track: Option<std::sync::Arc<racer_ai::AiTrack>>,
}

#[derive(Clone)]
pub struct GameplayCar {
    pub profile_id: String,
    pub source_fingerprint: String,
    pub collision: PreviewCollisionWorld,
}

impl GameplayDataStore {
    #[cfg(any(test, feature = "synthetic-multiplayer-test"))]
    pub fn synthetic_for_tests() -> Self {
        Self::synthetic_catalog(&SYNTHETIC_TRACK_IDS, &SYNTHETIC_CAR_IDS)
    }

    /// A catalogue of flat synthetic tracks and cars under the given profile
    /// ids (embedding builds pass their own ids).
    #[cfg(any(test, feature = "synthetic-multiplayer-test"))]
    pub fn synthetic_catalog(track_ids: &[&str], car_ids: &[&str]) -> Self {
        Self::synthetic_catalog_with_road(track_ids, car_ids, Vec::new())
    }

    /// Like [`Self::synthetic_catalog`], with flat ground under `edge_rows`
    /// (a road for the tracks' race course).
    #[cfg(any(test, feature = "synthetic-multiplayer-test"))]
    pub fn synthetic_catalog_with_road(
        track_ids: &[&str],
        car_ids: &[&str],
        edge_rows: Vec<racer_sim::TrackEdgeRow>,
    ) -> Self {
        let reach = edge_rows
            .iter()
            .fold(0.0_f64, |m, r| m.max(r.x.abs()).max(r.y.abs()))
            + 100.0;
        let collision = PreviewCollisionWorld {
            ground: vec![
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-reach, -reach, 0.0],
                        [reach, -reach, 0.0],
                        [reach, reach, 0.0],
                    ],
                },
                racer_sim::PreviewGroundTriangle {
                    vertices: [
                        [-reach, -reach, 0.0],
                        [reach, reach, 0.0],
                        [-reach, reach, 0.0],
                    ],
                },
            ],
            walls: Vec::new(),
            vehicle_radius: 1.0,
            vehicle_min_z: -0.5,
            vehicle_max_z: 0.5,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows,
        };
        let cars = car_ids
            .iter()
            .enumerate()
            .map(|(index, &profile_id)| {
                let source_fingerprint = char::from_digit(1 + index as u32, 16)
                    .unwrap_or('1')
                    .to_string()
                    .repeat(64);
                (
                    profile_id.to_owned(),
                    GameplayCar {
                        profile_id: profile_id.to_owned(),
                        source_fingerprint,
                        collision: collision.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let tracks = track_ids
            .iter()
            .enumerate()
            .map(|(index, &profile_id)| {
                let source_fingerprint = char::from_digit(3 + index as u32, 16)
                    .unwrap_or('3')
                    .to_string()
                    .repeat(64);
                let mut track = GameplayTrack {
                    profile_id: profile_id.to_owned(),
                    source_fingerprint,
                    cars: cars.clone(),
                    project: None,
                    course: None,
                };
                track.derive_course();
                (profile_id.to_owned(), std::sync::Arc::new(track))
            })
            .collect();
        Self {
            tracks,
            diagnostics: vec!["synthetic test catalog; no source assets".into()],
        }
    }

    /// Builds a catalog from tracks a private loader produced.
    pub fn from_parts(
        tracks: impl IntoIterator<Item = GameplayTrack>,
        diagnostics: Vec<String>,
    ) -> Self {
        Self {
            tracks: tracks
                .into_iter()
                .map(|mut track| {
                    track.derive_course();
                    (track.profile_id.clone(), std::sync::Arc::new(track))
                })
                .collect(),
            diagnostics,
        }
    }

    pub fn track(&self, id: &str) -> Option<&std::sync::Arc<GameplayTrack>> {
        self.tracks.get(id)
    }

    pub fn available_track_ids(&self) -> Vec<&str> {
        let mut ids = self.tracks.keys().map(String::as_str).collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub fn tracks(&self) -> Vec<&GameplayTrack> {
        let mut tracks = self.tracks.values().map(AsRef::as_ref).collect::<Vec<_>>();
        tracks.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        tracks
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}
