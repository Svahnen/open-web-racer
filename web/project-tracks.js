// Generated project tracks: catalog parsing and pose mapping helpers.
// The WASM client generates the tracks (racer-trackgen) and installs the scene;
// this module holds the pure page-side logic so it can be unit tested.

export const PROJECT_TRACK_PROFILE_PREFIX = "track-profile-rcp-";
/** Content identity of rooms on generated tracks (matches the server). */
export const PROJECT_TRACK_CONTENT_ID = "project-generated-tracks-v1";
const TRACK_ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,47}$/;

/** `track=test-circuit`: the bare project circuit chosen explicitly (without it the scenic default applies). */
export const TEST_CIRCUIT_TRACK_ID = "test-circuit";

/** The `track` URL parameter, or null when absent or not a valid track id. */
export function parseTrackParam(search) {
  const value = new URLSearchParams(search).get("track");
  return typeof value === "string" && TRACK_ID_PATTERN.test(value) ? value : null;
}

/** Track id of a `track-profile-rcp-<id>` profile id, or null. */
export function projectTrackIdFromProfile(profileId) {
  if (typeof profileId !== "string" || !profileId.startsWith(PROJECT_TRACK_PROFILE_PREFIX)) return null;
  const id = profileId.slice(PROJECT_TRACK_PROFILE_PREFIX.length);
  return TRACK_ID_PATTERN.test(id) ? id : null;
}

/** Validate the JSON catalog returned by the WASM client. */
export function parseCatalog(json) {
  let raw;
  try { raw = JSON.parse(json); } catch { return []; }
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((entry) => entry && TRACK_ID_PATTERN.test(String(entry.id)) && typeof entry.name === "string"
      && typeof entry.profile_id === "string" && entry.profile_id === `${PROJECT_TRACK_PROFILE_PREFIX}${entry.id}`
      && typeof entry.fingerprint === "string" && /^[0-9a-f]{64}$/.test(entry.fingerprint)
      && Number.isInteger(entry.laps) && entry.laps > 0)
    .map((entry) => ({
      id: entry.id,
      profileId: entry.profile_id,
      name: entry.name,
      description: typeof entry.description === "string" ? entry.description : "",
      theme: typeof entry.theme === "string" ? entry.theme : "",
      fingerprint: entry.fingerprint,
      laps: entry.laps,
    }));
}

/**
 * Catalog entry for a track id. Ids like `pine-ridge-s35` select the same road
 * with the start line moved to 35% of the lap (scenic spots for screenshots).
 */
export function resolveCatalogEntry(catalog, id) {
  const exact = catalog.find((entry) => entry.id === id);
  if (exact) return exact;
  const match = /^(.*)-s([0-9]{2})$/.exec(id);
  const base = match && catalog.find((entry) => entry.id === match[1]);
  return base ? { ...base, id, name: `${base.name} (start ${Number(match[2])}%)` } : null;
}

/** Link that starts a solo drive on `trackId` (null = the built-in circuit). */
export function soloTrackHref(trackId) {
  return trackId ? `?practice=local&track=${encodeURIComponent(trackId)}` : "?practice=local";
}

/**
 * Pose mapping from simulation to scene space for a track installed by the
 * WASM client. `metadata` is the JSON the client returned, `simSpawn` the
 * simulation pose of the spawn (slot 0 of the grid, the simulation origin).
 */
export function projectTrackPose(metadata, simSpawn) {
  const mapping = metadata?.spawn_pose_mapping;
  const origin = mapping?.anchor_scene_position_m;
  const forward = mapping?.anchor_scene_forward;
  const finite3 = (v) => Array.isArray(v) && v.length === 3 && v.every(Number.isFinite);
  if (!finite3(origin) || !finite3(forward) || Math.hypot(forward[0], forward[1]) < 1e-6) {
    throw new Error("project track has no usable spawn pose mapping");
  }
  return {
    simOrigin: [simSpawn.x, simSpawn.y],
    // simulation Z is already absolute scene height, so the Z mapping is the identity
    simOriginZ: origin[2],
    sceneOrigin: origin,
    headingOffset: Math.atan2(forward[1], forward[0]) - simSpawn.heading,
    unitsPerSimUnit: 1,
  };
}
