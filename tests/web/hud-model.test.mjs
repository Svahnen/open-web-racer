import test from "node:test";
import assert from "node:assert/strict";
import { PROVISIONAL_SIM_UNITS_PER_KMH, buildHudModel, buildRaceTimingModel, convertSpeed, buildRouteMapModel, createSessionClock, fitRouteMap, formatSessionTime, projectRouteMapMarker, setRouteMapFromSceneDescriptor } from "../../web/hud-model.js";

test("formats fixed simulation ticks as minutes, seconds, and centiseconds", () => {
  assert.equal(formatSessionTime(0), "0:00.00");
  assert.equal(formatSessionTime(2635), "0:43.91");
  assert.equal(formatSessionTime(3_721 * 60), "62:01.00");
  assert.equal(formatSessionTime(Number.NaN), "—");
  assert.equal(formatSessionTime(1, 0), "—");
});

test("session clock starts from observed active ticks and freezes only on authoritative finish", () => {
  const clock = createSessionClock();
  assert.equal(clock.update("race:9", 10, { running: false }), null);
  assert.equal(clock.update("race:9", 12, { running: true }), 0);
  assert.equal(clock.update("race:9", 72, { running: true }), 60);
  assert.equal(clock.update("race:9", 75, { finished: true }), 63);
  assert.equal(clock.update("race:9", 120, { running: false }), 63);
});

test("session clock resets across room and scene keys and does not infer a missed start", () => {
  const clock = createSessionClock();
  assert.equal(clock.update("room:1", 100, { running: true }), 0);
  assert.equal(clock.update("room:1", 160, { running: true }), 60);
  assert.equal(clock.update("room:2", 200, { running: true }), 0);
  assert.equal(clock.update("scene:next", 35, { running: true }), 0);
  assert.equal(clock.update("room:3", 250, { finished: true }), null);
});

test("session clock restarts if a local simulation tick counter rolls back", () => {
  const clock = createSessionClock();
  assert.equal(clock.update("scene:1", 500, { running: true }), 0);
  assert.equal(clock.update("scene:1", 560, { running: true }), 60);
  assert.equal(clock.update("scene:1", 4, { running: true }), 0);
  assert.equal(clock.update("scene:1", 64, { running: true }), 60);
});

test("open imported-track drive shows speed and session state without inventing lap progress", () => {
  const hud = buildHudModel({
    connected: false,
    localPracticeMode: true,
    openDrive: true,
    snapshot: { speed: 24.25, lap: 1 },
    elapsedTicks: 120,
  });
  assert.deepEqual(hud, { speed: "24.3", speedUnitLabel: "km/h", session: "0:02.00", state: "OPEN DRIVE", lap: null, race: null });
  assert.equal("position" in hud, false);
});

test("project speed display converts the same local and network values from km/h convention to mph", () => {
  const makeHud = (connected, speedUnit) => buildHudModel({
    connected,
    localPracticeMode: !connected,
    openDrive: true,
    snapshot: { speed: 100 },
    ownVehicle: { speed: 100 },
    networkStatus: "racing",
    inRoom: true,
    raceSummary: { laps: 2 },
    elapsedTicks: 0,
    speedUnit,
  });

  for (const connected of [false, true]) {
    assert.equal(makeHud(connected, "kmh").speed, "100.0");
    assert.equal(makeHud(connected, "mph").speed, "62.1");
  }
});

test("closed project circuit exposes its actual local lap while network lobby does not", () => {
  const local = buildHudModel({
    connected: false,
    localPracticeMode: true,
    openDrive: false,
    snapshot: { speed: 4, lap: 2 },
    elapsedTicks: 60,
    localLapTarget: 2,
  });
  assert.deepEqual(local.lap, { current: 2, target: 2 });

  const lobby = buildHudModel({
    connected: true,
    localPracticeMode: false,
    openDrive: false,
    snapshot: { speed: 99, lap: 2 },
    ownVehicle: { player_id: 7, speed: 8, completed_laps: 1 },
    networkStatus: "lobby",
    inRoom: true,
    raceSummary: { laps: 2 },
    elapsedTicks: null,
  });
  assert.equal(lobby.speed, "8.0");
  assert.equal(lobby.session, "—");
  assert.equal(lobby.state, "LOBBY");
  assert.equal(lobby.lap, null);
});

test("connected racing lap comes only from the own authoritative vehicle and declared target", () => {
  const hud = buildHudModel({
    connected: true,
    localPracticeMode: false,
    openDrive: false,
    snapshot: { speed: 90, lap: 9 },
    ownVehicle: { player_id: 3, speed: 12.5, completed_laps: 0 },
    networkStatus: "racing",
    inRoom: true,
    raceSummary: { laps: 3 },
    elapsedTicks: 180,
  });
  assert.deepEqual(hud, { speed: "12.5", speedUnitLabel: "km/h", session: "0:03.00", state: "RACING", lap: { current: 1, target: 3 }, race: null });
});

test("connected racing does not borrow another vehicle when the own snapshot is missing", () => {
  const hud = buildHudModel({
    connected: true,
    localPracticeMode: false,
    openDrive: false,
    snapshot: { speed: 99, lap: 9 },
    ownVehicle: null,
    networkStatus: "racing",
    inRoom: true,
    raceSummary: { laps: 3 },
    elapsedTicks: 90,
  });
  assert.equal(hud.speed, "—");
  assert.equal(hud.lap, null);
  assert.equal(hud.state, "RACING");
});

test("route map fits imported XY points without swapping axes and projects scene heading", () => {
  const model = buildRouteMapModel([[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]]);
  assert.ok(model);
  const fit = fitRouteMap(model, 100, 100, 10);
  assert.ok(fit);
  assert.deepEqual(Array.from(fit.points.slice(0, 8)), [10, 90, 90, 90, 90, 10, 10, 10]);
  const east = projectRouteMapMarker(fit, 5, 5, 0);
  const north = projectRouteMapMarker(fit, 5, 5, Math.PI / 2);
  assert.equal(east.inBounds, true);
  assert.equal(north.inBounds, true);
  assert.ok(east.tipX > east.x);
  assert.ok(Math.abs(east.tipY - east.y) < 1e-6);
  assert.ok(north.tipY < north.y);
  assert.ok(Math.abs(north.tipX - north.x) < 1e-6);
  assert.equal(projectRouteMapMarker(fit, 11, 5, 0).inBounds, false);
});

test("route map handles degenerate bounds and hides absent, malformed, or nonfinite paths", () => {
  const degenerate = buildRouteMapModel([[7, 7, 2], [7, 7, 2]]);
  const fit = fitRouteMap(degenerate, 112, 84, 6);
  assert.ok(fit);
  assert.ok(Array.from(fit.points).every(Number.isFinite));
  assert.equal(buildRouteMapModel(null), null);
  assert.equal(buildRouteMapModel([[1, 2, 3]]), null);
  assert.equal(buildRouteMapModel([[0, 0, 0], [Infinity, 1, 0]]), null);
  assert.equal(buildRouteMapModel([[0, 0, 0], [1_000_001, 0, 0]]), null);
  assert.equal(buildRouteMapModel(Array.from({ length: 65_536 }, () => [0, 0, 0])), null);
  assert.equal(fitRouteMap(degenerate, 10, 10, 6), null);
  assert.equal(projectRouteMapMarker(fit, NaN, 0, 0), null);
});

test("scene descriptor route wiring hides maps for route-free records", () => {
  let observed;
  const view = { setPath(path) { observed = path; return Array.isArray(path); } };
  const route = [[0, 0, 0], [1, 1, 0]];
  assert.equal(setRouteMapFromSceneDescriptor(view, { preview_reference_path_scene: route }), true);
  assert.equal(observed, route);
  assert.equal(setRouteMapFromSceneDescriptor(view, { schema_version: 0 }), false);
  assert.equal(observed, null);
});

test("raw speed unit bypasses the provisional conversion and is labeled", () => {
  assert.equal(PROVISIONAL_SIM_UNITS_PER_KMH, 1);
  assert.equal(convertSpeed(100, "raw"), 100);
  assert.equal(convertSpeed(100, "kmh"), 100);
  assert.equal(convertSpeed(Number.NaN, "kmh"), null);
  const hud = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 12.345 }, elapsedTicks: 0, speedUnit: "raw",
  });
  assert.equal(hud.speed, "12.3");
  assert.equal(hud.speedUnitLabel, "u/s");
  assert.equal(buildHudModel({ connected: false, openDrive: true, snapshot: { speed: 1 }, speedUnit: "mph" }).speedUnitLabel, "mph");
});

test("race timing model formats position, current, last, and best lap", () => {
  assert.deepEqual(
    buildRaceTimingModel({ race_position: 2, current_lap_ticks: 754, last_lap_ticks: 3600, best_lap_ticks: 3540 }, 4),
    { position: "P2/4", finished: false, lapTime: "0:12.56", lastLap: "1:00.00", bestLap: "0:59.00" },
  );
  const first = buildRaceTimingModel({ race_position: 1, current_lap_ticks: 0, last_lap_ticks: null, best_lap_ticks: undefined }, 1);
  assert.equal(first.lastLap, "—");
  assert.equal(first.bestLap, "—");
  const done = buildRaceTimingModel({ race_position: 3, finished: true, finish_ticks: 7200, current_lap_ticks: 0 }, 3);
  assert.equal(done.finished, true);
  assert.equal(done.lapTime, "2:00.00");
  assert.equal(buildRaceTimingModel({ race_position: 0 }, 4), null);
  assert.equal(buildRaceTimingModel({ race_position: 5 }, 4), null);
  assert.equal(buildRaceTimingModel({}, 4), null);
});

test("network and local HUD expose race timing only for ranked closed-circuit races", () => {
  const own = { race_position: 2, current_lap_ticks: 60, last_lap_ticks: 600, best_lap_ticks: 590, completed_laps: 1, speed: 5 };
  const net = buildHudModel({
    connected: true, ownVehicle: own, vehicles: [own, {}, {}], networkStatus: "racing",
    inRoom: true, raceSummary: { laps: 3 }, elapsedTicks: 0,
  });
  assert.equal(net.race.position, "P2/3");
  assert.equal(net.race.bestLap, "0:09.83");
  const lobby = buildHudModel({
    connected: true, ownVehicle: own, vehicles: [own], networkStatus: "lobby",
    inRoom: true, raceSummary: { laps: 3 }, elapsedTicks: 0,
  });
  assert.equal(lobby.race, null);
  const local = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: false,
    snapshot: { speed: 1, lap: 1, race_position: 1, current_lap_ticks: 120, last_lap_ticks: null, best_lap_ticks: null },
    elapsedTicks: 120,
  });
  assert.equal(local.race.position, "P1/1");
  assert.equal(local.race.lapTime, "0:02.00");
  const open = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 1, race_position: 1, current_lap_ticks: 120 }, elapsedTicks: 120,
  });
  assert.equal(open.race, null);
});

test("gearbox models report m/s speed and expose the gear; others are unchanged", () => {
  const local = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 30, gear: 2 }, elapsedTicks: 0, speedUnit: "kmh",
  });
  assert.equal(local.speed, "108.0");
  assert.equal(local.gear, "2");
  const mph = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 30, gear: 2 }, elapsedTicks: 0, speedUnit: "mph",
  });
  assert.equal(mph.speed, "67.1");
  const raw = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 30, gear: 2 }, elapsedTicks: 0, speedUnit: "raw",
  });
  assert.equal(raw.speed, "30.0");
  const network = buildHudModel({
    connected: true, localPracticeMode: false, openDrive: false, inRoom: true,
    ownVehicle: { player_id: 1, speed: 10, gear: 1 }, networkStatus: "open_drive", elapsedTicks: 0,
  });
  assert.equal(network.gear, "1");
  assert.equal(network.speed, "36.0");
  const none = buildHudModel({
    connected: false, localPracticeMode: true, openDrive: true,
    snapshot: { speed: 30, gear: 0 }, elapsedTicks: 0,
  });
  assert.equal(Object.hasOwn(none, "gear"), false);
  assert.equal(none.speed, "30.0");
});

test("reverse and neutral telemetry codes show R and N with a speed magnitude", () => {
  const base = { connected: false, localPracticeMode: true, openDrive: true, elapsedTicks: 0, speedUnit: "raw" };
  const reverse = buildHudModel({ ...base, snapshot: { speed: 12, gear: 250 } });
  assert.equal(reverse.gear, "R");
  assert.equal(reverse.speed, "12.0");
  const neutral = buildHudModel({ ...base, snapshot: { speed: 0, gear: 251 } });
  assert.equal(neutral.gear, "N");
});
