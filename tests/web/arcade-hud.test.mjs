import assert from "node:assert/strict";
import test from "node:test";
import {
  DIAL_START, DIAL_SWEEP, arcadeHudHook, buildArcadeModel, buildDrawList, buildTrackMap, computeLayout,
  createLapTracker, createWrongWayDetector, dialAngle, fitTrackMap, formatLapTime, paintDrawList, registerArcadeHud, speedScale,
} from "../../web/arcade-hud.js";
import { createPluginRegistry } from "../../web/plugins.js";

const base = { live: true, speed: 100, speedUnit: "km/h", gear: 3, rpm01: 0.5, transmission: "automatic", timing: { lastLapTicks: 5400, bestLapTicks: 5300 }, position: { rank: 3, entries: 8 }, lap: { completed: 1, total: 3 }, lapTicks: 1234, phase: 600, sinceStartTicks: 600 };
const circuit = { closed: true, smooth: true, centerline: [[-20, 0], [0, 12], [20, 0], [0, -12]], half_width: 5 };

test("lap time and dial math", () => {
  assert.equal(formatLapTime(null), "-:--.--");
  assert.equal(formatLapTime(-1), "-:--.--");
  assert.equal(formatLapTime(5400), "1:30.00");
  assert.equal(dialAngle(0), DIAL_START);
  assert.ok(Math.abs(dialAngle(1) - (DIAL_START + DIAL_SWEEP)) < 1e-12);
  assert.equal(dialAngle(5), dialAngle(1));
  assert.equal(dialAngle(Number.NaN), DIAL_START);
  assert.equal(speedScale("mph").max, 180);
  assert.equal(speedScale("km/h").max, 280);
});

test("model texts for a normal racing frame", () => {
  const m = buildArcadeModel(base);
  assert.equal(m.speedText, "100");
  assert.equal(m.gear, "3");
  assert.equal(m.transmissionText, "AUTO");
  assert.equal(m.positionText, "3/8");
  assert.equal(m.positionLabel, "P3/8");
  assert.equal(m.lapText, "2/3");
  assert.equal(m.timerText, "0:20.56");
  assert.equal(m.bestText, "1:28.33");
  assert.equal(m.lastText, "1:30.00");
  assert.equal(m.banner, "normal");
  assert.ok(Math.abs(m.speed01 - 100 / 280) < 1e-12);
  assert.equal(buildArcadeModel({ ...base, transmission: "manual" }).transmissionText, "MANUAL");
});

test("gear labels, empty frames and clamping", () => {
  assert.equal(buildArcadeModel({ ...base, gear: 250 }).gear, "R");
  assert.equal(buildArcadeModel({ ...base, gear: 250 }).gearKind, "reverse");
  assert.equal(buildArcadeModel({ ...base, gear: 251 }).gear, "N");
  assert.equal(buildArcadeModel({ ...base, gear: undefined }).gear, "-");
  const empty = buildArcadeModel({});
  assert.equal(empty.positionText, "");
  assert.equal(empty.lapText, "");
  assert.equal(empty.recordText, "-:--.--");
  assert.equal(empty.banner, "normal");
  const over = buildArcadeModel({ ...base, speed: 900, rpm01: 4, speedUnit: "mph" });
  assert.equal(over.speed01, 1);
  assert.equal(over.rpm01, 1);
  assert.equal(buildArcadeModel({ ...base, speed: -5 }).speedText, "0");
});

test("notice priority: countdown, go, wrong way, final lap, finished", () => {
  assert.equal(buildArcadeModel({ ...base, live: false, countdownTicks: 130, sinceStartTicks: null }).banner, "countdown");
  assert.equal(buildArcadeModel({ ...base, live: false, countdownTicks: 130 }).countdownDigit, "3");
  assert.equal(buildArcadeModel({ ...base, live: false, countdownTicks: 60 }).countdownDigit, "1");
  assert.equal(buildArcadeModel({ ...base, sinceStartTicks: 20 }).banner, "go");
  assert.equal(buildArcadeModel({ ...base, sinceStartTicks: 20, wrongWay: true }).banner, "go");
  assert.equal(buildArcadeModel({ ...base, wrongWay: true }).banner, "wrong-way");
  const finalLap = { ...base, lap: { completed: 2, total: 3 }, laps: { finalLapSince: 500 } };
  assert.equal(buildArcadeModel(finalLap).banner, "final-lap");
  assert.equal(buildArcadeModel(finalLap).lapText, "3/3");
  assert.equal(buildArcadeModel({ ...finalLap, phase: 500 + 240 }).banner, "normal");
  assert.equal(buildArcadeModel({ ...finalLap, wrongWay: true }).banner, "wrong-way");
  assert.equal(buildArcadeModel({ ...finalLap, finished: true }).banner, "finished");
});

test("finish results keep the player's row and cap the table", () => {
  const rows = Array.from({ length: 12 }, (_, i) => ({ position: i + 1, name: i === 10 ? "You" : `CPU ${i}`, ticks: i < 3 ? 5000 + i * 100 : null, you: i === 10 }));
  const m = buildArcadeModel({ ...base, finished: true, results: rows, lap: { completed: 3, total: 3 } });
  assert.equal(m.banner, "finished");
  assert.equal(m.timerLabel, "TOTAL");
  assert.equal(m.lapText, "3/3");
  assert.equal(m.results.length, 8);
  assert.ok(m.results.some((row) => row.you));
  assert.equal(m.results[0].time, "1:23.33");
  assert.equal(m.results[1].time, "1:25.00");
  assert.equal(m.results[3].time, "—");
  assert.deepEqual(buildArcadeModel({ ...base, results: rows }).results, []);
});

test("lap tracker records the best lap and the start of the final lap", () => {
  const tracker = createLapTracker();
  let state = tracker.update({ key: "a", phase: 100, completedLaps: 0, totalLaps: 3, bestLapTicks: null });
  assert.equal(state.finalLapSince, null);
  state = tracker.update({ key: "a", phase: 200, completedLaps: 2, totalLaps: 3, lastLapTicks: 900, bestLapTicks: 900 });
  assert.equal(state.finalLapSince, 200);
  state = tracker.update({ key: "a", phase: 300, completedLaps: 2, totalLaps: 3, lastLapTicks: 900, bestLapTicks: 950 });
  assert.equal(state.finalLapSince, 200);
  assert.equal(state.bestLapTicks, 900);
  state = tracker.update({ key: "b", phase: 10, completedLaps: 0, totalLaps: 1 });
  assert.equal(state.finalLapSince, null);
  assert.equal(state.bestLapTicks, null);
  assert.equal(tracker.update({ key: "b", phase: 20, completedLaps: 0, totalLaps: 1 }).finalLapSince, null);
});

test("wrong-way detector needs a sustained opposite heading above a minimum speed", () => {
  let clock = 0;
  const detector = createWrongWayDetector({ now: () => clock });
  const path = [[0, 0], [10, 0], [20, 0], [30, 0], [30, 10], [20, 10], [10, 10], [0, 10]];
  const drive = (heading, kmh) => detector.update(path, 10, 0, heading, kmh);
  assert.equal(drive(0, 50), false);
  assert.equal(drive(Math.PI, 50), false);
  clock += 300;
  assert.equal(drive(Math.PI, 50), false);
  clock += 400;
  assert.equal(drive(Math.PI, 50), true);
  clock += 100;
  assert.equal(drive(0, 50), true); // clears only after a short hold
  clock += 300;
  assert.equal(drive(0, 50), false);
  clock += 1000;
  assert.equal(drive(Math.PI, 2), false); // too slow to count
  assert.equal(detector.update(null, 0, 0, 0, 50), false);
  assert.equal(detector.update(path, Number.NaN, 0, 0, 50), false);
});

test("track map normalizes, smooths and fits", () => {
  const map = buildTrackMap(circuit);
  assert.ok(map.center.length > 20);
  assert.equal(map.left.length, map.center.length);
  assert.ok(Math.abs(map.width - 10) < 0.5);
  assert.ok(map.bounds.minX < -19 && map.bounds.maxX > 19);
  assert.equal(buildTrackMap({ centerline: [[0, 0], [1, 1]] }), null);
  assert.equal(buildTrackMap({ centerline: [[0, 0], [1, 1], [Number.NaN, 2]] }), null);
  assert.equal(buildTrackMap(null), null);
  const edged = buildTrackMap({ closed: true, centerline: [[0, 0], [10, 0], [10, 10]], left: [[0, 3], [10, 3], [7, 10]], right: [[0, -3], [10, -3], [13, 10]] });
  assert.deepEqual(edged.left[0], [0, 3]);
  const box = { x: 100, y: 200, w: 210, h: 180 };
  const fit = fitTrackMap(map, box, 16);
  for (const [x, y] of [...fit.center, ...fit.left, ...fit.right]) {
    assert.ok(x >= box.x && x <= box.x + box.w && y >= box.y && y <= box.y + box.h, `${x},${y} outside`);
  }
  const [north] = [fit.project(0, 12)];
  const [south] = [fit.project(0, -12)];
  assert.ok(north[1] < south[1], "north is up");
  assert.equal(fitTrackMap(null, box), null);
  assert.equal(fitTrackMap(map, { x: 0, y: 0, w: 0, h: 5 }), null);
});

test("layout scales and keeps elements apart", () => {
  const wide = computeLayout(1280, 720);
  assert.equal(wide.scale, 1);
  assert.equal(wide.compact, false);
  assert.ok(wide.banner.x > wide.cluster.x + wide.cluster.w, "banner clear of the gauges");
  for (const [w, h] of [[1280, 720], [960, 540], [640, 480], [390, 340], [360, 320], [1920, 1080]]) {
    const l = computeLayout(w, h);
    for (const name of ["banner", "cluster", "lapBox", "map"]) {
      const r = l[name];
      assert.ok(r.x >= 0 && r.y >= 0 && r.x + r.w <= l.vw + 1e-9 && r.y + r.h <= l.vh + 1e-9, `${name} fits ${w}x${h}`);
    }
    assert.ok(l.scale >= 0.55);
    // bottom-left box and bottom-right map never overlap
    assert.ok(l.lapBox.x + l.lapBox.w <= l.map.x, `lap box and map apart at ${w}x${h}`);
    // the gauges never collide with the banner
    const clusterBelow = l.cluster.y >= l.banner.y + l.banner.h;
    const clusterLeft = l.cluster.x + l.cluster.w <= l.banner.x;
    assert.ok(clusterBelow || clusterLeft, `gauges clear of banner at ${w}x${h}`);
  }
  assert.equal(computeLayout(360, 320).compact, true);
});

const numbers = (value, out = []) => {
  if (typeof value === "number") out.push(value);
  else if (Array.isArray(value)) value.forEach((item) => numbers(item, out));
  else if (value && typeof value === "object") Object.values(value).forEach((item) => numbers(item, out));
  return out;
};
const texts = (ops) => ops.filter((op) => op.op === "text").map((op) => op.text);

test("draw list shows the gauges, banner, lap box and map", () => {
  const layout = computeLayout(1280, 720);
  const map = buildTrackMap(circuit);
  const model = buildArcadeModel(base);
  const ops = buildDrawList(model, layout, { trackMap: map, player: { x: 0, y: 12, heading: 0 }, rivalPoses: [{ x: -20, y: 0, slot: 0 }, { x: 20, y: 0, slot: 1 }] });
  const t = texts(ops);
  for (const expected of ["100", "KM/H", "3", "AUTO", "0:20.56", "P3", "/8", "2/3", "1:28.33", "1:30.00", "LAP TIME", "BEST", "LAST"]) {
    assert.ok(t.includes(expected), `missing text ${expected}`);
  }
  assert.ok(t.includes("280") && t.includes("0"), "speed dial labels");
  const speedNeedle = ops.find((op) => op.tag === "speed");
  const angle = Math.atan2(speedNeedle.y2 - speedNeedle.y1, speedNeedle.x2 - speedNeedle.x1);
  const expected = dialAngle(100 / 280);
  assert.ok(Math.abs(Math.atan2(Math.sin(angle - expected), Math.cos(angle - expected))) < 1e-9, "needle follows speed");
  assert.equal(ops.filter((op) => op.tag === "rival").length, 2);
  const player = ops.find((op) => op.tag === "player");
  const box = layout.map;
  for (const [x, y] of player.points) assert.ok(x >= box.x && x <= box.x + box.w && y >= box.y && y <= box.y + box.h);
  assert.ok(numbers(ops.map((op) => ({ ...op, text: undefined, fill: undefined, stroke: undefined, shadow: undefined }))).every(Number.isFinite), "no NaN coordinates");
});

test("draw list without a map or pose still draws the rest", () => {
  const ops = buildDrawList(buildArcadeModel({ ...base, position: null, lap: null }), computeLayout(390, 340), {});
  assert.ok(texts(ops).includes("NO MAP"));
  assert.equal(ops.filter((op) => op.tag === "player").length, 0);
  assert.ok(texts(ops).includes("—"));
});

test("notices and the results table appear as drawn text", () => {
  const layout = computeLayout(1280, 720);
  const countdown = buildDrawList(buildArcadeModel({ ...base, live: false, countdownTicks: 100 }), layout, {});
  assert.ok(countdown.some((op) => op.tag === "countdown" && op.text === "2"));
  const go = buildDrawList(buildArcadeModel({ ...base, sinceStartTicks: 10 }), layout, {});
  assert.ok(go.some((op) => op.tag === "go" && op.text === "GO!"));
  const wrong = buildDrawList(buildArcadeModel({ ...base, wrongWay: true }), layout, {});
  assert.ok(texts(wrong).includes("WRONG WAY"));
  const final = buildDrawList(buildArcadeModel({ ...base, lap: { completed: 2, total: 3 }, laps: { finalLapSince: 590 } }), layout, {});
  assert.ok(texts(final).includes("FINAL LAP"));
  const rows = [{ position: 1, name: "CPU 1", ticks: 5000 }, { position: 2, name: "You", ticks: 5100, you: true }, { position: 3, name: "CPU 2", ticks: null }];
  const done = buildDrawList(buildArcadeModel({ ...base, finished: true, results: rows, lap: { completed: 3, total: 3 } }), layout, {});
  assert.ok(texts(done).includes("RACE COMPLETE"));
  assert.deepEqual(done.filter((op) => op.tag === "result-name").map((op) => op.text), ["CPU 1", "You", "CPU 2"]);
  assert.equal(done.filter((op) => op.tag === "results-you").length, 1);
  assert.equal(buildDrawList(buildArcadeModel(base), layout, {}).some((op) => op.tag === "results"), false);
  for (const ops of [countdown, go, wrong, final, done]) {
    assert.ok(numbers(ops.map((op) => ({ ...op, text: undefined, fill: undefined, stroke: undefined, shadow: undefined }))).every(Number.isFinite));
  }
});

test("painter runs every op type on a context", () => {
  const calls = [];
  const ctx = new Proxy({}, {
    get: (_, name) => {
      if (name === "createLinearGradient") return () => ({ addColorStop: () => calls.push("stop") });
      return (...args) => calls.push(String(name));
    },
    set: (_, name) => { calls.push(`set:${String(name)}`); return true; },
  });
  const ops = buildDrawList(buildArcadeModel({ ...base, finished: true, results: [{ position: 1, name: "You", ticks: 1, you: true }] }), computeLayout(1280, 720), { trackMap: buildTrackMap(circuit), player: { x: 0, y: 0, heading: 1 } });
  paintDrawList(ctx, ops);
  for (const call of ["fillText", "arc", "roundRect", "stroke", "fill", "lineTo", "save", "restore"]) assert.ok(calls.includes(call), call);
});

test("registered at the lowest priority so a plugin HUD wins", () => {
  const registry = createPluginRegistry({ warn: () => {} });
  registerArcadeHud(registry);
  assert.equal(registry.first("hud"), arcadeHudHook);
  assert.equal(arcadeHudHook.builtin, true);
  const plugin = { createRenderer() {}, buildModel() {} };
  registry.register({ id: "private-hud", hooks: { hud: plugin } });
  assert.equal(registry.first("hud"), plugin);
  registry.register({ id: "late-low", priority: -5, hooks: { hud: {} } });
  assert.equal(registry.first("hud"), plugin);
  assert.deepEqual(registry.providers("hud").map((p) => p.pluginId), ["private-hud", "late-low", "owr-arcade-hud"]);
});
