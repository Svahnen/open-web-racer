//! Project-owned demo tracks: generated at run time by `racer-trackgen`,
//! passed through the generic RCP provider and turned into engine tracks.
//!
//! Generation is deterministic and takes well under a second, which is why
//! the packs are not committed as binary files (`docs/TRACKGEN.md`).

use crate::provider::{IngestError, ProviderRegistry};
use crate::rcp_track::{RcpTrackScene, build_track_scene};
use racer_trackgen::{builtin_specs, find_spec, generate_files};

/// Content identity shared by all generated tracks in room/identity checks.
pub const PROJECT_TRACK_CONTENT_ID: &str = "project-generated-tracks-v1";
/// Profile id prefix of generated tracks (`track-profile-rcp-<id>`).
pub const PROJECT_TRACK_PROFILE_PREFIX: &str = "track-profile-rcp-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectTrackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub theme: &'static str,
    /// Hex SHA-256 of the spec the track is generated from.
    pub fingerprint: String,
    pub laps: u32,
}

/// The selectable generated tracks, in menu order.
pub fn project_track_catalog() -> Vec<ProjectTrackInfo> {
    builtin_specs()
        .into_iter()
        .map(|s| ProjectTrackInfo {
            fingerprint: s.fingerprint(),
            id: s.id.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            theme: s.theme.name(),
            laps: s.laps,
        })
        .collect()
}

/// Profile id for a generated track id.
pub fn project_track_profile_id(id: &str) -> String {
    format!("{PROJECT_TRACK_PROFILE_PREFIX}{id}")
}

/// Inverse of [`project_track_profile_id`].
pub fn project_track_id_from_profile(profile: &str) -> Option<&str> {
    profile.strip_prefix(PROJECT_TRACK_PROFILE_PREFIX)
}

/// Catalog entry for a track or variant id (`-mirror`, `-reverse` suffixes).
pub fn project_track_info(id: &str) -> Option<ProjectTrackInfo> {
    let s = find_spec(id)?;
    Some(ProjectTrackInfo {
        fingerprint: s.fingerprint(),
        id: s.id.clone(),
        name: s.name.clone(),
        description: s.description.clone(),
        theme: s.theme.name(),
        laps: s.laps,
    })
}

/// Generate `id`, ingest it through the generic RCP provider and build the
/// render scene and simulation inputs.
pub fn load_project_track(id: &str) -> Result<RcpTrackScene, String> {
    let spec = find_spec(id).ok_or_else(|| format!("unknown project track {id:?}"))?;
    let files = generate_files(&spec);
    let pack = ProviderRegistry::with_native()
        .ingest(&files)
        .map_err(|e: IngestError| e.to_string())?;
    build_track_scene(&pack, 0)
}
