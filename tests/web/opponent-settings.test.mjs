import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_OPPONENTS,
  MAX_OPPONENTS,
  clampLaps,
  clampOpponents,
  clampSkill,
  decodeOpponentStates,
  opponentSummary,
  opponentVoices,
  parseOpponentParams,
  projectRenderPoses,
  soloQuery,
} from "../../web/opponent-settings.js";
import { buildHudModel } from "../../web/hud-model.js";
import { remoteCarsForAudio } from "../../web/audio-feed.js";

test("opponent counts clamp to the supported range and default to seven", () => {
  assert.equal(DEFAULT_OPPONENTS, 7);
  assert.equal(MAX_OPPONENTS, 7);
  assert.equal(clampOpponents("3"), 3);
  assert.equal(clampOpponents(99), 7);
  assert.equal(clampOpponents(-4), 0);
  assert.equal(clampOpponents("abc"), 7);
  assert.equal(clampOpponents("", 0), 0);
  assert.equal(clampOpponents(2.9), 2);
  assert.equal(clampSkill("0"), 0);
  assert.equal(clampSkill("1"), 1);
  assert.equal(clampSkill("nonsense"), 1);
});

test("solo settings round-trip through the query string and old URLs keep no opponents", () => {
  assert.deepEqual(parseOpponentParams("?practice=local"), { count: 0, skill: 1, catchUp: true, laps: 1 });
  const settings = { count: 3, skill: 0, catchUp: false, laps: 1 };
  const query = soloQuery(settings);
  assert.equal(query, "practice=local&ai=3&skill=0&catchup=0");
  assert.deepEqual(parseOpponentParams(`?${query}`), settings);
  assert.deepEqual(parseOpponentParams("?practice=local&ai=12&skill=5"), { count: 7, skill: 1, catchUp: true, laps: 1 });
});

test("laps on installed tracks: 0 is free driving, 1 to 5 race, anything else is the default", () => {
  assert.equal(clampLaps("0"), 0);
  assert.equal(clampLaps("3"), 3);
  assert.equal(clampLaps(99), 5);
  assert.equal(clampLaps(-2), 0);
  assert.equal(clampLaps("abc"), 1);
  assert.equal(clampLaps(""), 1);
  assert.equal(parseOpponentParams("?practice=local&ai=2&laps=0").laps, 0);
  assert.equal(parseOpponentParams("?practice=local&ai=2&laps=4").laps, 4);
  assert.match(soloQuery({ count: 2, skill: 1, catchUp: true, laps: 0 }), /&laps=0$/);
  assert.match(soloQuery({ count: 2, skill: 1, catchUp: true, laps: 3 }), /&laps=3$/);
  assert.doesNotMatch(soloQuery({ count: 2, skill: 1, catchUp: true, laps: 1 }), /laps/);
});

test("the summary names count, difficulty and catch-up", () => {
  assert.equal(opponentSummary({ count: 0, skill: 1, catchUp: true }), "No computer opponents");
  assert.equal(opponentSummary({ count: 1, skill: 0, catchUp: false }), "1 computer opponent · Beginner · catch-up off");
  assert.equal(opponentSummary({ count: 7, skill: 1, catchUp: true }), "7 computer opponents · Advanced · catch-up on");
});

test("opponent states decode and become render poses and voices", () => {
  const flat = new Float64Array([1000, 1, 2, 0.5, 3, 4, 5, 1, 0, 2, 1001, -1, -2, 1.5, 0, 0, 0, 0, 1, 3]);
  const states = decodeOpponentStates(flat);
  assert.equal(states.length, 2);
  assert.deepEqual(states[0], { id: 1000, x: 1, y: 2, heading: 0.5, vx: 3, vy: 4, speed: 5, laps: 1, finished: false, position: 2 });
  assert.equal(states[1].finished, true);
  assert.deepEqual(decodeOpponentStates(new Float64Array(7)), []);
  const { poses, signature, localIndex } = projectRenderPoses({ x: 9, y: 8, heading: 0.25 }, states);
  assert.equal(poses.length, 9);
  assert.equal(localIndex, 0);
  assert.equal(signature, "project-circuit:3");
  assert.deepEqual([...poses.slice(0, 3)], [9, 8, 0.25]);
  const voices = remoteCarsForAudio(opponentVoices(states), null, { x: 0, y: 0, heading: 0 }, () => null);
  assert.equal(voices.length, 2);
  assert.ok(voices.every((voice) => voice.rpm01 > 0 && voice.rpm01 <= 1));
});

test("the local HUD ranks against every competitor", () => {
  const snapshot = { speed: 5, lap: 1, race_position: 3, competitors: 8, current_lap_ticks: 60 };
  const model = buildHudModel({ connected: false, localPracticeMode: true, openDrive: false, snapshot, localCompetitors: 8, elapsedTicks: 0 });
  assert.equal(model.race.position, "P3/8");
  const solo = buildHudModel({ connected: false, localPracticeMode: true, openDrive: false, snapshot: { ...snapshot, race_position: 1 }, elapsedTicks: 0 });
  assert.equal(solo.race.position, "P1/1");
});

test("solo query carries a valid generated track id only", () => {
  assert.equal(soloQuery({ count: 3, skill: 1, catchUp: true, track: "pine-ridge" }), "practice=local&ai=3&skill=1&catchup=1&track=pine-ridge");
  assert.equal(soloQuery({ count: 3, skill: 1, catchUp: true, track: "../x" }), "practice=local&ai=3&skill=1&catchup=1");
  assert.equal(soloQuery({ count: 3, skill: 1, catchUp: true, track: null }), "practice=local&ai=3&skill=1&catchup=1");
});
