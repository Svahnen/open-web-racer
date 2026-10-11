// Built-in race HUD for the public build: analog speedometer and tachometer with a gear
// indicator, a top banner (lap time, position, lap), best/last lap box, a mini track map,
// countdown, wrong-way and final-lap notices and a finish results table.
//
// Everything is drawn by code on a 2D canvas; there are no image or font files. The
// model and layout functions are pure: `buildArcadeModel` turns the frame values into
// texts and levels, `computeLayout` scales the design to the canvas, and
// `buildDrawList` produces a flat list of drawing operations that `paintDrawList` runs
// on a canvas context. Registered as the lowest-priority `hud` provider (see
// docs/PLUGINS.md), so any plugin that provides a HUD takes over.

import { formatSessionTime, gearLabel } from "./hud-model.js";

export const ARCADE_HUD_ID = "owr-arcade-hud";
export const ARCADE_ART_KIND = "arcade";

const TAU = Math.PI * 2;
export const DIAL_START = Math.PI * 0.75;
export const DIAL_SWEEP = Math.PI * 1.5;

const GEAR_R = 250;
const GEAR_N = 251;
const TICKS_PER_SECOND = 60;
const GO_TICKS = 75;
const FINAL_LAP_TICKS = 240;
const MAX_RESULT_ROWS = 8;

const COLOR = Object.freeze({
  panel: "#0a1020",
  panelEdge: "#2b3a5c",
  ink: "#f4f7ff",
  dim: "#8d9bb5",
  amber: "#ffb400",
  cyan: "#2de2e6",
  red: "#ff3b4e",
  green: "#6dff7a",
  road: "#27324d",
  roadEdge: "#9fb0c8",
});
export const RIVAL_COLORS = Object.freeze(["#ff4d5e", "#35d6ff", "#ffd23f", "#b86bff", "#4dff9a", "#ff8a3d", "#ffffff"]);

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
const finite = (value) => typeof value === "number" && Number.isFinite(value);

// ---------------------------------------------------------------------------
// Formatting and dial math

/** `m:ss.cc` for 60 Hz ticks, or `-:--.--` when unknown. */
export function formatLapTime(ticks) {
  return finite(ticks) && ticks >= 0 ? formatSessionTime(ticks) : "-:--.--";
}

/** Canvas angle (radians, clockwise from +x) of a dial position 0..1. */
export function dialAngle(value01) {
  return DIAL_START + clamp(finite(value01) ? value01 : 0, 0, 1) * DIAL_SWEEP;
}

/** Dial range and tick spacing for a speed unit label ("km/h", "mph" or "u/s"). */
export function speedScale(unit) {
  if (unit === "mph") return { max: 180, major: 30, minorPerMajor: 3 };
  return { max: 280, major: 40, minorPerMajor: 4 };
}

function dialColor(fraction) {
  if (fraction >= 0.9) return COLOR.red;
  if (fraction >= 0.72) return COLOR.amber;
  return COLOR.cyan;
}

// ---------------------------------------------------------------------------
// Model

/**
 * Frame values -> display model. All fields are plain strings and numbers.
 * `frame`: {live, speed, speedUnit, gear, rpm01, transmission, timing, position,
 * lap, lapTicks, phase, wrongWay, recordTicks, countdownTicks, sinceStartTicks,
 * finished, results, laps}
 */
export function buildArcadeModel(frame = {}) {
  const live = frame.live !== false;
  const unit = frame.speedUnit === "mph" || frame.speedUnit === "u/s" ? frame.speedUnit : "km/h";
  const scale = speedScale(unit);
  const speed = finite(frame.speed) ? Math.max(0, frame.speed) : 0;
  const rpm01 = clamp(finite(frame.rpm01) ? frame.rpm01 : 0, 0, 1);
  const hasGear = Number.isInteger(frame.gear) && frame.gear > 0;
  const finished = frame.finished === true;
  const lap = frame.lap && Number.isInteger(frame.lap.total) && frame.lap.total > 0 ? frame.lap : null;
  const lapNumber = lap ? (finished ? lap.total : Math.min(lap.total, Math.max(0, frame.lap.completed) + 1)) : null;
  const position = frame.position && Number.isInteger(frame.position.rank) ? frame.position : null;
  const phase = finite(frame.phase) ? frame.phase : 0;

  const countdownTicks = finite(frame.countdownTicks) && frame.countdownTicks > 0 ? frame.countdownTicks : null;
  const sinceStart = finite(frame.sinceStartTicks) ? frame.sinceStartTicks : null;
  const goAge = live && !finished && countdownTicks === null && sinceStart !== null && sinceStart >= 0 && sinceStart < GO_TICKS ? sinceStart : null;
  const finalSince = finite(frame.laps?.finalLapSince) ? frame.laps.finalLapSince : null;
  const finalAge = finalSince !== null && phase >= finalSince ? phase - finalSince : null;
  const finalLapActive = finalAge !== null && finalAge < FINAL_LAP_TICKS && !finished;

  let banner = "normal";
  if (finished) banner = "finished";
  else if (countdownTicks !== null) banner = "countdown";
  else if (goAge !== null) banner = "go";
  else if (frame.wrongWay === true) banner = "wrong-way";
  else if (finalLapActive) banner = "final-lap";

  const timing = frame.timing ?? null;
  const lastTicks = timing?.lastLapTicks ?? null;
  const bestTicks = timing?.bestLapTicks ?? frame.recordTicks ?? null;

  const results = (Array.isArray(frame.results) ? frame.results : []).map((row, index) => ({
    pos: String(Number.isInteger(row.position) ? row.position : index + 1),
    name: String(row.name ?? "").slice(0, 14),
    time: finite(row.ticks) && row.ticks >= 0 ? formatLapTime(row.ticks) : "—",
    you: row.you === true,
  }));
  const visibleResults = results.slice(0, MAX_RESULT_ROWS);
  if (!visibleResults.some((row) => row.you)) {
    const mine = results.find((row) => row.you);
    if (mine) visibleResults[visibleResults.length - 1] = mine;
  }

  return {
    live,
    banner,
    speedValue: speed,
    speedText: String(Math.round(speed)),
    speedUnitText: unit,
    speed01: clamp(speed / scale.max, 0, 1),
    scale,
    rpm01,
    tachLevel: Math.round(rpm01 * 100) / 100,
    gear: hasGear ? gearLabel(frame.gear) : "-",
    gearKind: !hasGear ? "none" : frame.gear === GEAR_R ? "reverse" : frame.gear === GEAR_N ? "neutral" : "forward",
    transmissionText: frame.transmission === "manual" ? "MANUAL" : "AUTO",
    timerLabel: finished ? "TOTAL" : "LAP TIME",
    timerText: timing ? formatLapTime(frame.lapTicks) : "-:--.--",
    positionRank: position ? String(position.rank) : "",
    positionTotal: position ? String(position.entries) : "",
    positionText: position ? `${position.rank}/${position.entries}` : "",
    lapText: lap ? `${lapNumber}/${lap.total}` : "",
    lapNumber,
    lapTotal: lap ? lap.total : null,
    lastText: formatLapTime(lastTicks),
    bestText: formatLapTime(bestTicks),
    recordText: formatLapTime(frame.recordTicks ?? bestTicks),
    rivalName: String(frame.rivalName ?? ""),
    countdownTicks,
    countdownDigit: countdownTicks !== null ? String(Math.ceil(countdownTicks / TICKS_PER_SECOND)) : "",
    goAge,
    finalLapAge: finalLapActive ? finalAge : null,
    wrongWay: frame.wrongWay === true && !finished && countdownTicks === null,
    blink: Math.floor(phase / 12) % 2 === 0,
    finished,
    results: finished ? visibleResults : [],
    // exposed under these names for the page's data attributes
    speedUnit: unit,
    positionLabel: position ? `P${position.rank}/${position.entries}` : "",
  };
}

// ---------------------------------------------------------------------------
// Lap tracker and wrong-way detector (the `hud` hook's helpers)

/** Remembers best lap and when the final lap began. Phase is in 60 Hz ticks. */
export function createLapTracker() {
  let currentKey = null;
  let finalLapSince = null;
  let lastPhase = null;
  let best = null;
  return {
    update({ key, phase, completedLaps = null, totalLaps = null, lastLapTicks = null, bestLapTicks = null } = {}) {
      if (key !== currentKey || (lastPhase !== null && finite(phase) && phase < lastPhase)) {
        currentKey = key;
        finalLapSince = null;
        best = null;
      }
      lastPhase = finite(phase) ? phase : lastPhase;
      const final = Number.isInteger(completedLaps) && Number.isInteger(totalLaps) && totalLaps > 1 && completedLaps === totalLaps - 1;
      if (!final) finalLapSince = null;
      else if (finalLapSince === null && finite(phase)) finalLapSince = phase;
      if (finite(bestLapTicks)) best = best === null ? bestLapTicks : Math.min(best, bestLapTicks);
      return { finalLapSince, lastLapTicks, bestLapTicks: best, storedRecord: best };
    },
  };
}

/**
 * Decides whether the car is driving against the track direction. `path` is the
 * ordered centerline `[[x, y, ...], ...]` of a closed loop; `heading` is the direction
 * of travel. Needs a sustained opposite heading above a minimum speed.
 */
export function createWrongWayDetector({ now = () => Date.now(), onMs = 600, offMs = 250, minKmh = 8 } = {}) {
  let hint = 0;
  let badSince = null;
  let goodSince = null;
  let active = false;
  const reset = () => { hint = 0; badSince = null; goodSince = null; active = false; };
  return {
    reset,
    update(path, x, y, heading, kmh) {
      if (!Array.isArray(path) || path.length < 3 || ![x, y, heading].every(finite)) { reset(); return false; }
      const count = path.length;
      let bestIndex = hint % count;
      let bestDist = Infinity;
      for (let i = 0; i < count; i++) {
        const d = (path[i][0] - x) ** 2 + (path[i][1] - y) ** 2;
        if (d < bestDist) { bestDist = d; bestIndex = i; }
      }
      hint = bestIndex;
      const a = path[(bestIndex + count - 1) % count], b = path[(bestIndex + 1) % count];
      const tx = b[0] - a[0], ty = b[1] - a[1];
      const length = Math.hypot(tx, ty);
      const against = length > 1e-6 && (Math.cos(heading) * tx + Math.sin(heading) * ty) / length < -0.3 && finite(kmh) && kmh > minKmh;
      const t = now();
      if (against) {
        goodSince = null;
        badSince ??= t;
        if (t - badSince >= onMs) active = true;
      } else {
        badSince = null;
        goodSince ??= t;
        if (t - goodSince >= offMs) active = false;
      }
      return active;
    },
  };
}

// ---------------------------------------------------------------------------
// Track map

function smoothClosed(points, steps) {
  const out = [];
  const n = points.length;
  for (let i = 0; i < n; i++) {
    const p0 = points[(i + n - 1) % n], p1 = points[i], p2 = points[(i + 1) % n], p3 = points[(i + 2) % n];
    for (let s = 0; s < steps; s++) {
      const t = s / steps, t2 = t * t, t3 = t2 * t;
      out.push([0, 1].map((k) => 0.5 * (2 * p1[k] + (-p0[k] + p2[k]) * t
        + (2 * p0[k] - 5 * p1[k] + 4 * p2[k] - p3[k]) * t2 + (-p0[k] + 3 * p1[k] - 3 * p2[k] + p3[k]) * t3)));
    }
  }
  return out;
}

function offsetEdges(center, halfWidth, closed) {
  const n = center.length;
  const left = [], right = [];
  for (let i = 0; i < n; i++) {
    const a = center[closed ? (i + n - 1) % n : Math.max(0, i - 1)];
    const b = center[closed ? (i + 1) % n : Math.min(n - 1, i + 1)];
    const length = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
    const nx = -(b[1] - a[1]) / length, ny = (b[0] - a[0]) / length;
    left.push([center[i][0] + nx * halfWidth, center[i][1] + ny * halfWidth]);
    right.push([center[i][0] - nx * halfWidth, center[i][1] - ny * halfWidth]);
  }
  return { left, right };
}

const validPoint = (p) => Array.isArray(p) && p.length >= 2 && finite(p[0]) && finite(p[1]);

/**
 * Normalizes a track outline `{centerline, left?, right?, half_width?, closed?, smooth?}`
 * (metres, any consistent frame) into `{center, left, right, closed, bounds, width}`, or null.
 */
export function buildTrackMap(raw) {
  if (!raw || !Array.isArray(raw.centerline) || raw.centerline.length < 3 || !raw.centerline.every(validPoint)) return null;
  const closed = raw.closed !== false;
  let center = raw.centerline.map((p) => [p[0], p[1]]);
  let left, right;
  if (raw.smooth) {
    center = smoothClosed(center, 10);
  }
  const edgesOk = !raw.smooth && Array.isArray(raw.left) && Array.isArray(raw.right)
    && raw.left.length === center.length && raw.right.length === center.length
    && raw.left.every(validPoint) && raw.right.every(validPoint);
  if (edgesOk) {
    left = raw.left.map((p) => [p[0], p[1]]);
    right = raw.right.map((p) => [p[0], p[1]]);
  } else {
    const half = finite(raw.half_width) && raw.half_width > 0 ? raw.half_width : 6;
    ({ left, right } = offsetEdges(center, half, closed));
  }
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const edge of [left, right, center]) {
    for (const [x, y] of edge) {
      minX = Math.min(minX, x); maxX = Math.max(maxX, x);
      minY = Math.min(minY, y); maxY = Math.max(maxY, y);
    }
  }
  let widthSum = 0;
  for (let i = 0; i < center.length; i++) widthSum += Math.hypot(left[i][0] - right[i][0], left[i][1] - right[i][1]);
  return { center, left, right, closed, bounds: { minX, minY, maxX, maxY }, width: widthSum / center.length };
}

/** Fits a track map into `box` {x, y, w, h}; north (+y) is up. Returns projected polylines and `project(x, y)`. */
export function fitTrackMap(map, box, pad = 12) {
  if (!map || !(box?.w > 0) || !(box?.h > 0)) return null;
  const { minX, minY, maxX, maxY } = map.bounds;
  const spanX = Math.max(maxX - minX, 1e-3), spanY = Math.max(maxY - minY, 1e-3);
  const scale = Math.min((box.w - 2 * pad) / spanX, (box.h - 2 * pad) / spanY);
  if (!(scale > 0)) return null;
  const ox = box.x + (box.w - spanX * scale) / 2;
  const oy = box.y + (box.h - spanY * scale) / 2;
  const project = (x, y) => [ox + (x - minX) * scale, oy + (maxY - y) * scale];
  const mapAll = (points) => points.map((p) => project(p[0], p[1]));
  return { scale, project, center: mapAll(map.center), left: mapAll(map.left), right: mapAll(map.right), roadPx: map.width * scale };
}

// ---------------------------------------------------------------------------
// Layout

/** Design size 1280x720; scales up and down with the canvas, compact below ~1180 design units wide. */
export function computeLayout(width, height) {
  const scale = clamp(Math.min(width / 1280, height / 720), 0.55, 2.2);
  const vw = width / scale, vh = height / scale;
  const compact = vw < 1180;
  const margin = 16;
  const banner = { w: compact ? Math.min(480, vw - 24) : 480, h: 64, y: compact ? 10 : 14 };
  banner.x = (vw - banner.w) / 2;
  const cluster = { x: margin, y: compact ? banner.y + banner.h + 10 : margin, w: 312, h: 184 };
  const lapBox = { x: margin, y: vh - margin - 84, w: 214, h: 84 };
  const mapSize = compact ? { w: 150, h: 132 } : { w: 210, h: 180 };
  const map = { x: vw - margin - mapSize.w, y: vh - margin - mapSize.h, ...mapSize };
  return { scale, vw, vh, compact, banner, cluster, lapBox, map, center: { x: vw / 2, y: vh * 0.36 } };
}

// ---------------------------------------------------------------------------
// Draw list

const poly = (points, o = {}) => ({ op: "poly", points, closed: true, ...o });
const line = (x1, y1, x2, y2, o = {}) => ({ op: "line", x1, y1, x2, y2, ...o });
const arc = (cx, cy, r, a0, a1, o = {}) => ({ op: "arc", cx, cy, r, a0, a1, ...o });
const circle = (cx, cy, r, o = {}) => ({ op: "circle", cx, cy, r, ...o });
const rrect = (x, y, w, h, r, o = {}) => ({ op: "rrect", x, y, w, h, r, ...o });
const text = (x, y, value, size, o = {}) => ({ op: "text", x, y, text: String(value), size, ...o });
const vertical = (y0, y1, from, to) => ({ x0: 0, y0, x1: 0, y1, stops: [[0, from], [1, to]] });

function pushDial(ops, d) {
  const { cx, cy, r } = d;
  ops.push(circle(cx, cy, r + 5, { fill: "#05070f", stroke: COLOR.amber, lineWidth: 3, shadow: { blur: 10, color: "#000" } }));
  ops.push(circle(cx, cy, r, { fill: { x0: 0, y0: cy - r, x1: 0, y1: cy + r, stops: [[0, "#1a2542"], [1, "#070b17"]] } }));
  const track = r - 10;
  ops.push(arc(cx, cy, track, DIAL_START, DIAL_START + DIAL_SWEEP, { stroke: "#1e2a47", lineWidth: 9, cap: "butt" }));
  if (d.redFrom01 !== undefined) {
    ops.push(arc(cx, cy, track, dialAngle(d.redFrom01), dialAngle(1), { stroke: COLOR.red, lineWidth: 9, cap: "butt", alpha: 0.9 }));
  }
  if (d.value01 > 0.003) {
    ops.push(arc(cx, cy, track, DIAL_START, dialAngle(d.value01), { stroke: d.valueColor, lineWidth: 9, cap: "butt", shadow: { blur: 8, color: d.valueColor } }));
  }
  const majors = d.labels.length - 1;
  for (let i = 0; i <= majors; i++) {
    for (let m = 0; m < d.minorPerMajor && i < majors; m++) {
      if (m === 0) continue;
      const a = dialAngle((i + m / d.minorPerMajor) / majors);
      ops.push(line(cx + Math.cos(a) * (track - 8), cy + Math.sin(a) * (track - 8), cx + Math.cos(a) * (track - 14), cy + Math.sin(a) * (track - 14), { stroke: COLOR.dim, lineWidth: 1.5 }));
    }
    const a = dialAngle(i / majors);
    ops.push(line(cx + Math.cos(a) * (track - 8), cy + Math.sin(a) * (track - 8), cx + Math.cos(a) * (track - 19), cy + Math.sin(a) * (track - 19), { stroke: COLOR.ink, lineWidth: 2.5 }));
    ops.push(text(cx + Math.cos(a) * (track - 31), cy + Math.sin(a) * (track - 31), d.labels[i], d.labelSize, { align: "center", baseline: "middle", fill: COLOR.ink, font: "mono", weight: 700 }));
  }
  const a = dialAngle(d.value01);
  ops.push(line(cx - Math.cos(a) * 14, cy - Math.sin(a) * 14, cx + Math.cos(a) * (track - 4), cy + Math.sin(a) * (track - 4), { stroke: COLOR.red, lineWidth: 4, cap: "round", shadow: { blur: 6, color: "#000" }, tag: d.needleTag }));
  ops.push(circle(cx, cy, 7, { fill: "#d8dff0", stroke: "#05070f", lineWidth: 2 }));
}

function pushGauges(ops, model, cluster) {
  const speedo = { cx: cluster.x + 92, cy: cluster.y + 92, r: 92 };
  const tach = { cx: cluster.x + 242, cy: cluster.y + 114, r: 70 };
  const tachLabels = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];
  pushDial(ops, {
    ...tach, value01: model.rpm01, valueColor: dialColor(model.rpm01 / 0.86 * 0.8), redFrom01: 0.86,
    labels: tachLabels, minorPerMajor: 2, labelSize: 11, needleTag: "tach",
  });
  const { max, major } = model.scale;
  const labels = [];
  for (let v = 0; v <= max + 1e-9; v += major) labels.push(String(v));
  pushDial(ops, {
    ...speedo, value01: model.speed01, valueColor: dialColor(model.speed01), labels, minorPerMajor: model.scale.minorPerMajor,
    labelSize: 13, needleTag: "speed",
  });
  ops.push(text(speedo.cx, speedo.cy + 55, model.speedText, 30, { align: "center", baseline: "middle", fill: COLOR.ink, font: "mono", weight: 800, italic: true, stroke: "#000", strokeWidth: 4, tag: "speed-text" }));
  ops.push(text(speedo.cx, speedo.cy + 76, model.speedUnitText.toUpperCase(), 11, { align: "center", baseline: "middle", fill: COLOR.amber, font: "display", weight: 800 }));
  // gear window on the tachometer face
  const gearColor = model.gearKind === "reverse" ? COLOR.red : model.gearKind === "neutral" ? COLOR.green : COLOR.ink;
  ops.push(text(tach.cx, tach.cy - 20, "x1000 RPM", 8, { align: "center", baseline: "middle", fill: COLOR.dim, font: "display", weight: 700 }));
  ops.push(rrect(tach.cx - 20, tach.cy - 12, 40, 44, 6, { fill: "#05070f", stroke: COLOR.panelEdge, lineWidth: 2 }));
  ops.push(text(tach.cx, tach.cy + 10, model.gear, 36, { align: "center", baseline: "middle", fill: gearColor, font: "display", weight: 900, italic: true, tag: "gear" }));
  ops.push(rrect(tach.cx - 24, tach.cy + 36, 48, 16, 8, { fill: model.transmissionText === "MANUAL" ? COLOR.amber : "#1e2a47", stroke: COLOR.panelEdge, lineWidth: 1.5 }));
  ops.push(text(tach.cx, tach.cy + 44.5, model.transmissionText, 10, { align: "center", baseline: "middle", fill: model.transmissionText === "MANUAL" ? "#150f00" : COLOR.ink, font: "display", weight: 800, tag: "transmission" }));
}

function pushBanner(ops, model, b) {
  const slant = 14;
  const plate = [[b.x + slant, b.y], [b.x + b.w, b.y], [b.x + b.w - slant, b.y + b.h], [b.x, b.y + b.h]];
  ops.push(poly(plate, { fill: vertical(b.y, b.y + b.h, "#16213f", "#080c19"), stroke: COLOR.amber, lineWidth: 2.5, alpha: 0.94, shadow: { blur: 8, color: "#000" } }));
  const cell = b.w / 3 + 20;
  const timerX = b.x + 22, posX = b.x + cell + 6, lapX = b.x + b.w - 88;
  ops.push(text(timerX, b.y + 15, model.timerLabel, 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
  ops.push(text(timerX, b.y + 41, model.timerText, 32, { fill: COLOR.ink, font: "mono", weight: 800, baseline: "middle", tag: "timer" }));
  ops.push(line(posX - 14, b.y + 10, posX - 20, b.y + b.h - 10, { stroke: COLOR.panelEdge, lineWidth: 2 }));
  ops.push(text(posX, b.y + 15, "POS", 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
  if (model.positionRank) {
    ops.push(text(posX, b.y + 41, `P${model.positionRank}`, 34, { fill: COLOR.amber, font: "display", weight: 900, italic: true, baseline: "middle", tag: "position" }));
    ops.push(text(posX + 14 + (model.positionRank.length + 1) * 23, b.y + 46, `/${model.positionTotal}`, 18, { fill: COLOR.ink, font: "display", weight: 800, baseline: "middle" }));
  } else {
    ops.push(text(posX, b.y + 41, "—", 30, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle", tag: "position" }));
  }
  ops.push(line(lapX - 16, b.y + 10, lapX - 22, b.y + b.h - 10, { stroke: COLOR.panelEdge, lineWidth: 2 }));
  ops.push(text(lapX, b.y + 15, "LAP", 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
  ops.push(text(lapX, b.y + 41, model.lapText || "—", 28, { fill: model.finalLapAge !== null && model.blink ? COLOR.amber : COLOR.ink, font: "mono", weight: 800, baseline: "middle", tag: "lap" }));
}

function pushLapBox(ops, model, box) {
  ops.push(rrect(box.x, box.y, box.w, box.h, 10, { fill: vertical(box.y, box.y + box.h, "#16213f", "#080c19"), stroke: COLOR.panelEdge, lineWidth: 2, alpha: 0.92 }));
  ops.push(rect4(box.x, box.y + 10, 4, box.h - 20, COLOR.amber));
  const rows = [["BEST", model.bestText, COLOR.green], ["LAST", model.lastText, COLOR.ink]];
  rows.forEach(([label, value, color], i) => {
    const y = box.y + 24 + i * 36;
    ops.push(text(box.x + 18, y, label, 12, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
    ops.push(text(box.x + box.w - 14, y, value, 24, { fill: color, font: "mono", weight: 800, baseline: "middle", align: "right", tag: label.toLowerCase() }));
  });
}

function rect4(x, y, w, h, fill) {
  return rrect(x, y, w, h, 2, { fill });
}

function pushMap(ops, model, mapBox, extras) {
  ops.push(rrect(mapBox.x, mapBox.y, mapBox.w, mapBox.h, 10, { fill: vertical(mapBox.y, mapBox.y + mapBox.h, "#101a33", "#070a15"), stroke: COLOR.panelEdge, lineWidth: 2, alpha: 0.9 }));
  const fit = fitTrackMap(extras?.trackMap, mapBox, 16);
  if (!fit) {
    ops.push(text(mapBox.x + mapBox.w / 2, mapBox.y + mapBox.h / 2, "NO MAP", 12, { fill: COLOR.dim, font: "display", weight: 800, align: "center", baseline: "middle" }));
    return;
  }
  const roadWidth = clamp(fit.roadPx, 5, 11);
  ops.push({ op: "poly", points: fit.center, closed: extras.trackMap.closed, stroke: COLOR.roadEdge, lineWidth: roadWidth + 3, join: "round", cap: "round", tag: "map-outline" });
  ops.push({ op: "poly", points: fit.center, closed: extras.trackMap.closed, stroke: COLOR.road, lineWidth: roadWidth, join: "round", cap: "round" });
  // start/finish bar across the road at the first centerline point
  const [p0, p1] = [fit.center[0], fit.center[1]];
  const dx = p1[0] - p0[0], dy = p1[1] - p0[1], len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len, ny = dx / len, half = roadWidth / 2 + 3;
  ops.push(line(p0[0] - nx * half, p0[1] - ny * half, p0[0] + nx * half, p0[1] + ny * half, { stroke: COLOR.ink, lineWidth: 3 }));
  (extras.rivalPoses ?? []).forEach((rival) => {
    if (!finite(rival.x) || !finite(rival.y)) return;
    const [x, y] = fit.project(rival.x, rival.y);
    ops.push(circle(x, y, 4, { fill: RIVAL_COLORS[(rival.slot ?? 0) % RIVAL_COLORS.length], stroke: "#05070f", lineWidth: 1.5, tag: "rival" }));
  });
  const player = extras.player;
  if (player && [player.x, player.y, player.heading].every(finite)) {
    const [x, y] = fit.project(player.x, player.y);
    const dirX = Math.cos(player.heading), dirY = -Math.sin(player.heading);
    const tip = [x + dirX * 9, y + dirY * 9];
    const a = [x - dirX * 5 - dirY * 5, y - dirY * 5 + dirX * 5];
    const b = [x - dirX * 5 + dirY * 5, y - dirY * 5 - dirX * 5];
    ops.push(circle(x, y, 8, { stroke: COLOR.amber, lineWidth: 1.5, alpha: model.blink ? 0.9 : 0.35 }));
    ops.push(poly([tip, a, b], { fill: COLOR.amber, stroke: "#05070f", lineWidth: 1.5, join: "round", tag: "player" }));
  }
}

function pushNotices(ops, model, layout) {
  const { x, y } = layout.center;
  if (model.banner === "countdown") {
    const frac = (model.countdownTicks % TICKS_PER_SECOND) / TICKS_PER_SECOND;
    const size = 190 * (1 + (1 - frac) * 0.0 + frac * 0.35);
    ops.push(text(x, y, model.countdownDigit, size, { align: "center", baseline: "middle", fill: model.countdownDigit === "1" ? COLOR.green : COLOR.amber, stroke: "#000", strokeWidth: 10, font: "display", weight: 900, italic: true, alpha: clamp(0.35 + frac * 0.9, 0, 1), tag: "countdown" }));
  } else if (model.banner === "go") {
    const age = model.goAge / GO_TICKS;
    ops.push(text(x, y, "GO!", 190 * (1 + age * 0.25), { align: "center", baseline: "middle", fill: COLOR.green, stroke: "#000", strokeWidth: 10, font: "display", weight: 900, italic: true, alpha: clamp(1.4 - age * 1.4, 0, 1), tag: "go" }));
  } else if (model.banner === "wrong-way") {
    const w = 360, h = 78, top = y - h / 2;
    ops.push(poly([[x - w / 2 + 14, top], [x + w / 2, top], [x + w / 2 - 14, top + h], [x - w / 2, top + h]], { fill: COLOR.red, stroke: COLOR.ink, lineWidth: 3, alpha: model.blink ? 0.95 : 0.55, tag: "wrong-way" }));
    ops.push(text(x, y, "WRONG WAY", 52, { align: "center", baseline: "middle", fill: COLOR.ink, font: "display", weight: 900, italic: true, alpha: model.blink ? 1 : 0.7 }));
  } else if (model.banner === "final-lap") {
    const slide = clamp(model.finalLapAge / 18, 0, 1);
    const offset = (1 - slide) * -240;
    const alpha = model.finalLapAge > FINAL_LAP_TICKS - 30 ? clamp((FINAL_LAP_TICKS - model.finalLapAge) / 30, 0, 1) : 1;
    const w = 300, h = 56, top = y - h / 2;
    ops.push(poly([[x - w / 2 + 12 + offset, top], [x + w / 2 + offset, top], [x + w / 2 - 12 + offset, top + h], [x - w / 2 + offset, top + h]], { fill: "#150f00", stroke: COLOR.amber, lineWidth: 3, alpha: 0.92 * alpha, tag: "final-lap" }));
    ops.push(text(x + offset, y, "FINAL LAP", 38, { align: "center", baseline: "middle", fill: COLOR.amber, font: "display", weight: 900, italic: true, alpha }));
  }
}

function pushResults(ops, model, layout) {
  const rows = model.results;
  if (!model.finished) return;
  const rowH = 32, w = 480, headH = 92, footH = 18;
  const h = headH + Math.max(1, rows.length) * rowH + footH;
  const x = layout.vw / 2 - w / 2, y = clamp(layout.vh / 2 - h / 2, layout.banner.y + layout.banner.h + 16, layout.vh);
  ops.push(rrect(x, y, w, h, 14, { fill: vertical(y, y + h, "#16213f", "#070a15"), stroke: COLOR.amber, lineWidth: 3, alpha: 0.95, shadow: { blur: 16, color: "#000" }, tag: "results" }));
  ops.push(text(x + w / 2, y + 26, "RACE COMPLETE", 30, { align: "center", baseline: "middle", fill: COLOR.amber, font: "display", weight: 900, italic: true }));
  ops.push(text(x + w / 2, y + 56, `TOTAL ${model.timerText}   BEST ${model.bestText}`, 14, { align: "center", baseline: "middle", fill: COLOR.ink, font: "mono", weight: 700 }));
  const head = y + headH - 8;
  ops.push(text(x + 28, head, "POS", 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
  ops.push(text(x + 90, head, "DRIVER", 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle" }));
  ops.push(text(x + w - 28, head, "TIME", 11, { fill: COLOR.dim, font: "display", weight: 800, baseline: "middle", align: "right" }));
  rows.forEach((row, i) => {
    const ry = y + headH + i * rowH;
    if (row.you) ops.push(rrect(x + 12, ry, w - 24, rowH - 4, 6, { fill: COLOR.amber, alpha: 0.22, tag: "results-you" }));
    const color = row.you ? COLOR.amber : COLOR.ink;
    ops.push(text(x + 28, ry + rowH / 2 - 2, row.pos, 20, { fill: color, font: "mono", weight: 800, baseline: "middle", tag: "result-pos" }));
    ops.push(text(x + 90, ry + rowH / 2 - 2, row.name, 20, { fill: color, font: "display", weight: 800, baseline: "middle", tag: "result-name" }));
    ops.push(text(x + w - 28, ry + rowH / 2 - 2, row.time, 20, { fill: color, font: "mono", weight: 800, baseline: "middle", align: "right", tag: "result-time" }));
  });
}

/**
 * Drawing operations in design units for one frame. `extras`: {trackMap (from
 * `buildTrackMap`), player {x, y, heading}, rivalPoses [{x, y, slot}]} in the map's frame.
 */
export function buildDrawList(model, layout, extras = {}) {
  const ops = [];
  pushGauges(ops, model, layout.cluster);
  pushBanner(ops, model, layout.banner);
  pushLapBox(ops, model, layout.lapBox);
  pushMap(ops, model, layout.map, extras);
  pushNotices(ops, model, layout);
  pushResults(ops, model, layout);
  return ops;
}

// ---------------------------------------------------------------------------
// Painting

const FONT_FAMILY = {
  mono: "\"Cascadia Mono\", \"SF Mono\", Consolas, \"DejaVu Sans Mono\", \"Liberation Mono\", monospace",
  display: "\"Arial Black\", \"Segoe UI\", \"Helvetica Neue\", Arial, \"Liberation Sans\", sans-serif",
};

function resolvePaint(ctx, paint) {
  if (paint && typeof paint === "object") {
    const gradient = ctx.createLinearGradient(paint.x0, paint.y0, paint.x1, paint.y1);
    for (const [at, color] of paint.stops) gradient.addColorStop(at, color);
    return gradient;
  }
  return paint;
}

function tracePoly(ctx, points, closed) {
  ctx.beginPath();
  points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  if (closed) ctx.closePath();
}

/** Runs a draw list on a 2D context (design units; the caller sets the base transform). */
export function paintDrawList(ctx, ops) {
  for (const op of ops) {
    ctx.save();
    ctx.globalAlpha = op.alpha ?? 1;
    if (op.shadow) { ctx.shadowBlur = op.shadow.blur; ctx.shadowColor = op.shadow.color; }
    ctx.lineJoin = op.join ?? "miter";
    ctx.lineCap = op.cap ?? "butt";
    switch (op.op) {
      case "poly":
        tracePoly(ctx, op.points, op.closed);
        break;
      case "rrect":
        ctx.beginPath();
        ctx.roundRect(op.x, op.y, op.w, op.h, op.r);
        break;
      case "arc":
        ctx.beginPath();
        ctx.arc(op.cx, op.cy, op.r, op.a0, op.a1);
        break;
      case "circle":
        ctx.beginPath();
        ctx.arc(op.cx, op.cy, op.r, 0, TAU);
        break;
      case "line":
        ctx.beginPath();
        ctx.moveTo(op.x1, op.y1);
        ctx.lineTo(op.x2, op.y2);
        break;
      case "text":
        ctx.font = `${op.italic ? "italic " : ""}${op.weight ?? 700} ${op.size}px ${FONT_FAMILY[op.font] ?? FONT_FAMILY.display}`;
        ctx.textAlign = op.align ?? "left";
        ctx.textBaseline = op.baseline ?? "alphabetic";
        ctx.lineJoin = "round";
        if (op.stroke) {
          ctx.lineWidth = op.strokeWidth ?? 3;
          ctx.strokeStyle = op.stroke;
          ctx.strokeText(op.text, op.x, op.y);
        }
        ctx.fillStyle = resolvePaint(ctx, op.fill ?? COLOR.ink);
        ctx.fillText(op.text, op.x, op.y);
        ctx.restore();
        continue;
      default:
        ctx.restore();
        continue;
    }
    if (op.fill) { ctx.fillStyle = resolvePaint(ctx, op.fill); ctx.fill(); }
    if (op.stroke) {
      ctx.shadowBlur = op.fill ? 0 : (op.shadow?.blur ?? 0);
      ctx.lineWidth = op.lineWidth ?? 1;
      ctx.strokeStyle = op.stroke;
      ctx.stroke();
    }
    ctx.restore();
  }
}

/** Renderer for the `hud` hook: sizes the canvas to its box and draws the list each frame. */
export function createArcadeRenderer({ canvas } = {}) {
  const context = canvas?.getContext?.("2d") ?? null;
  if (canvas?.style) canvas.style.imageRendering = "auto";
  return {
    artKind: ARCADE_ART_KIND,
    setArt() {},
    render(model, extras) {
      if (!context || !model) return;
      const rect = canvas.getBoundingClientRect();
      if (!(rect.width > 0) || !(rect.height > 0)) return;
      const dpr = Math.min(globalThis.window?.devicePixelRatio || 1, 2);
      const pixelW = Math.min(2048, Math.round(rect.width * dpr));
      const pixelH = Math.min(2048, Math.round(rect.height * dpr));
      if (canvas.width !== pixelW || canvas.height !== pixelH) { canvas.width = pixelW; canvas.height = pixelH; }
      const layout = computeLayout(rect.width, rect.height);
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, pixelW, pixelH);
      const k = (pixelW / rect.width) * layout.scale;
      context.setTransform(k, 0, 0, k, 0, 0);
      paintDrawList(context, buildDrawList(model, layout, extras));
    },
  };
}

/** The `hud` hook object of the built-in HUD. */
export const arcadeHudHook = {
  builtin: true,
  createRenderer: createArcadeRenderer,
  createLapTracker,
  createWrongWayDetector,
  buildModel: buildArcadeModel,
  toTicks: (ticks) => ticks,
  classifyPath: () => null,
};

/** Registers the built-in HUD at the lowest priority, so any plugin that provides a `hud` overrides it. */
export function registerArcadeHud(registry) {
  return registry.register({ id: ARCADE_HUD_ID, version: "1.0.0", priority: -1000, hooks: { hud: arcadeHudHook } });
}
