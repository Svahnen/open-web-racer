import assert from "node:assert/strict";
import test from "node:test";
import { PROJECT_TRACK_CONTENT_ID, parseCatalog, parseTrackParam, projectTrackIdFromProfile, projectTrackPose, resolveCatalogEntry, soloTrackHref } from "../../web/project-tracks.js";

const fingerprint = "a".repeat(64);
const entry = (id) => ({ id, profile_id: `track-profile-rcp-${id}`, name: id.toUpperCase(), description: "d", theme: "t", fingerprint, laps: 3 });

test("track parameter accepts plain ids only", () => {
  assert.equal(parseTrackParam("?practice=local&track=pine-ridge"), "pine-ridge");
  assert.equal(parseTrackParam("?practice=local"), null);
  assert.equal(parseTrackParam("?track=../etc"), null);
  assert.equal(parseTrackParam("?track=Pine"), null);
  assert.equal(parseTrackParam(`?track=${"a".repeat(60)}`), null);
});

test("catalog parsing keeps only well-formed entries", () => {
  const good = entry("red-mesa");
  const bad = [{ ...entry("bad-profile"), profile_id: "other" }, { ...entry("short-print"), fingerprint: "abc" }, { ...entry("no-laps"), laps: 0 }, { id: 5 }, null];
  const parsed = parseCatalog(JSON.stringify([good, ...bad]));
  assert.deepEqual(parsed.map((t) => t.id), ["red-mesa"]);
  assert.equal(parsed[0].profileId, "track-profile-rcp-red-mesa");
  assert.deepEqual(parseCatalog("not json"), []);
  assert.deepEqual(parseCatalog("{}"), []);
});

test("spot variants resolve to their base track", () => {
  const catalog = parseCatalog(JSON.stringify([entry("pine-ridge"), entry("red-mesa")]));
  assert.equal(resolveCatalogEntry(catalog, "red-mesa").id, "red-mesa");
  const spot = resolveCatalogEntry(catalog, "pine-ridge-s35");
  assert.equal(spot.id, "pine-ridge-s35");
  assert.match(spot.name, /start 35%/);
  assert.equal(resolveCatalogEntry(catalog, "unknown-s10"), null);
  assert.equal(resolveCatalogEntry(catalog, "unknown"), null);
});

test("room profile ids map back to track ids", () => {
  assert.equal(projectTrackIdFromProfile("track-profile-rcp-night-harbour"), "night-harbour");
  assert.equal(projectTrackIdFromProfile("track-profile-project-circuit"), null);
  assert.equal(projectTrackIdFromProfile("track-profile-rcp-"), null);
  assert.equal(projectTrackIdFromProfile("track-profile-rcp-../x"), null);
  assert.equal(projectTrackIdFromProfile(null), null);
  assert.equal(PROJECT_TRACK_CONTENT_ID, "project-generated-tracks-v1");
});

test("solo links encode the track", () => {
  assert.equal(soloTrackHref(null), "?practice=local");
  assert.equal(soloTrackHref("night-harbour"), "?practice=local&track=night-harbour");
});

test("pose mapping follows the anchor and forward direction", () => {
  const metadata = { spawn_pose_mapping: { anchor_scene_position_m: [10, 20, 3], anchor_scene_forward: [0, 1, 0] } };
  const pose = projectTrackPose(metadata, { x: 0, y: 0, heading: 0 });
  assert.deepEqual(pose.sceneOrigin, [10, 20, 3]);
  assert.ok(Math.abs(pose.headingOffset - Math.PI / 2) < 1e-12);
  assert.equal(pose.simOriginZ, 3);
  assert.equal(pose.unitsPerSimUnit, 1);
  assert.throws(() => projectTrackPose({}, { x: 0, y: 0, heading: 0 }), /spawn pose/);
  assert.throws(() => projectTrackPose({ spawn_pose_mapping: { anchor_scene_position_m: [0, 0, 0], anchor_scene_forward: [0, 0, 1] } }, { x: 0, y: 0, heading: 0 }), /spawn pose/);
});
