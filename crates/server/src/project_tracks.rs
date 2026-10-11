//! Generated project tracks as selectable room tracks.
//!
//! Tracks come from `racer-trackgen` through the generic RCP ingest path and
//! are cached after first use; the project default car drives them.

use crate::gameplay_data::{GameplayCar, GameplayTrack, ProjectRuntime};
use racer_content::project_tracks::{
    PROJECT_TRACK_CONTENT_ID, load_project_track, project_track_catalog,
    project_track_id_from_profile, project_track_profile_id,
};
use racer_sim::Vec2;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

pub const PROJECT_VEHICLE_PROFILE_ID: &str = "vehicle-profile-project-default";

type Cache = Mutex<HashMap<String, Arc<GameplayTrack>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// True for profile ids that name a generated track (valid or not).
pub fn is_project_profile(profile_id: &str) -> bool {
    project_track_id_from_profile(profile_id).is_some()
}

/// Fingerprint (hex SHA-256 of the generating spec) of a catalog track.
pub fn project_fingerprint(profile_id: &str) -> Option<String> {
    let id = project_track_id_from_profile(profile_id)?;
    project_track_catalog()
        .into_iter()
        .find(|t| t.id == id)
        .map(|t| t.fingerprint)
}

/// The generated track for `profile_id`, generating it on first use. Only
/// tracks of the catalog are available.
pub fn project_gameplay_track(profile_id: &str) -> Option<Arc<GameplayTrack>> {
    let id = project_track_id_from_profile(profile_id)?;
    let info = project_track_catalog().into_iter().find(|t| t.id == id)?;
    let mut guard = cache().lock().ok()?;
    if let Some(track) = guard.get(profile_id) {
        return Some(Arc::clone(track));
    }
    let loaded = load_project_track(&info.id).ok()?;
    let frames = racer_ai::race::road_frames_from_edge_rows(&loaded.collision.edge_rows, 1.0);
    let ai_track = racer_ai::AiTrack::from_pack_track_frames(
        &loaded.pack_track,
        &frames,
        &racer_ai::DeriveParams::default(),
    )
    .ok()
    .map(Arc::new);
    let car = GameplayCar {
        profile_id: PROJECT_VEHICLE_PROFILE_ID.to_owned(),
        source_fingerprint: "0".repeat(64),
        collision: loaded.collision.clone(),
    };
    let mut cars = HashMap::new();
    cars.insert(car.profile_id.clone(), car);
    let track = Arc::new(GameplayTrack {
        profile_id: project_track_profile_id(&info.id),
        source_fingerprint: info.fingerprint,
        cars,
        course: None,
        project: Some(ProjectRuntime {
            runtime: loaded.runtime.clone(),
            ai_track,
            spawn_slots: loaded
                .spawn_slots
                .iter()
                .map(|s| Vec2::new(s.x, s.y))
                .collect(),
        }),
    });
    guard.insert(profile_id.to_owned(), Arc::clone(&track));
    Some(track)
}

/// Validates a create/change/join selection against the catalog.
pub fn resolve_project_selection(
    profile_id: &str,
    fingerprint: &str,
    content_identity: &str,
    laps: u8,
) -> Result<Arc<GameplayTrack>, &'static str> {
    if content_identity != PROJECT_TRACK_CONTENT_ID || !(1..=5).contains(&laps) {
        return Err("content_mismatch");
    }
    let expected = project_fingerprint(profile_id).ok_or("profile_unavailable")?;
    if fingerprint != expected {
        return Err("content_mismatch");
    }
    project_gameplay_track(profile_id).ok_or("profile_unavailable")
}

/// Generate and cache every catalog track (call once at startup so the first
/// room on a track does not wait for generation).
pub fn warm() {
    for track in project_track_catalog() {
        let _ = project_gameplay_track(&project_track_profile_id(&track.id));
    }
}
