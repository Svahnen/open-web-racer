import { RacerPlugins } from "./plugins.js";

const DEFAULT_HANDLING_LABELS = Object.freeze({ 0: "handling: provisional", 1: "handling: calibrated", 4: "handling: arcade", 6: "handling: classic" });

/** Label for a protocol `handling_model` id: built-in ids first, then the `vehicleModel` hook; null when unknown. */
function handlingLabel(id) {
  return DEFAULT_HANDLING_LABELS[id] ?? RacerPlugins.invoke("vehicleModel", "handlingLabel", [id], null);
}

export function createSessionClock() {
  let currentKey = null;
  let startTick = null;
  let endTick = null;
  let lastTick = null;

  return {
    update(key, tick, { running, finished = false } = {}) {
      if (key !== currentKey) {
        currentKey = key;
        startTick = null;
        endTick = null;
        lastTick = null;
      }
      if (!Number.isFinite(tick) || tick < 0) return null;
      const currentTick = Math.floor(tick);
      if (lastTick !== null && currentTick < lastTick && running) {
        startTick = currentTick;
        endTick = null;
      }
      if (running && startTick === null) startTick = currentTick;
      if (finished && startTick !== null && endTick === null) endTick = currentTick;
      lastTick = currentTick;
      if (startTick === null) return null;
      return Math.max(0, (endTick ?? currentTick) - startTick);
    },
  };
}

export function formatSessionTime(ticks, ticksPerSecond = 60) {
  if (!Number.isFinite(ticks) || ticks < 0
    || !Number.isFinite(ticksPerSecond) || ticksPerSecond <= 0) return "—";
  const centiseconds = Math.floor(ticks * 100 / ticksPerSecond);
  const minutes = Math.floor(centiseconds / 6000);
  const seconds = Math.floor(centiseconds / 100) % 60;
  const fraction = centiseconds % 100;
  return `${minutes}:${String(seconds).padStart(2, "0")}.${String(fraction).padStart(2, "0")}`;
}

// PROVISIONAL: the reference speed scale is unknown. The project convention
// treats one simulation speed unit as one km/h. Change only this constant
// (and its test) if a measured scale replaces the placeholder.
export const PROVISIONAL_SIM_UNITS_PER_KMH = 1;
export const KMH_PER_MPH = 1.609344;
export const SPEED_UNIT_LABELS = { kmh: "km/h", mph: "mph", raw: "u/s" };

// The calibrated vehicle model (imported profiles) reports speed in the
// observed m/s-like unit (HUD mph = speed * 2.2369), so its
// snapshots carry a gearbox gear > 0 and convert with 3.6 instead.
export const KMH_PER_MPS = 3.6;

export function convertSpeed(rawSpeed, unit = "kmh", { metersPerSecond = false } = {}) {
  if (!Number.isFinite(rawSpeed)) return null;
  if (unit === "raw") return Number(rawSpeed);
  const kmh = metersPerSecond
    ? Number(rawSpeed) * KMH_PER_MPS
    : Number(rawSpeed) / PROVISIONAL_SIM_UNITS_PER_KMH;
  return unit === "mph" ? kmh / KMH_PER_MPH : kmh;
}

const optionalTicks = (ticks) => (Number.isFinite(ticks) && ticks >= 0 ? ticks : null);

// Race position and lap timing as shown on the HUD. Inputs are authoritative
// (network) or local-sim values in 60 Hz race ticks; `total` is the number of
// ranked competitors.
export function buildRaceTimingModel(vehicle, total) {
  const position = vehicle?.race_position;
  if (!Number.isInteger(position) || position < 1
    || !Number.isInteger(total) || total < position) return null;
  const finishTicks = optionalTicks(vehicle.finish_ticks);
  const finished = vehicle.finished === true || finishTicks !== null;
  const fmt = (ticks) => (optionalTicks(ticks) === null ? "—" : formatSessionTime(ticks));
  return {
    position: `P${position}/${total}`,
    finished,
    lapTime: finished ? fmt(finishTicks) : fmt(vehicle.current_lap_ticks),
    lastLap: fmt(vehicle.last_lap_ticks),
    bestLap: fmt(vehicle.best_lap_ticks),
  };
}

function statusKind(status) {
  if (typeof status === "string") return status.toLowerCase();
  if (status?.countdown) return "countdown";
  return Object.keys(status ?? {})[0]?.toLowerCase() ?? "";
}

// Gearbox telemetry codes: 250 is reverse ("R"), 251 is neutral ("N"),
// 1..5 are forward gears (observed gearbox telemetry). Speed is always a magnitude.
export function gearLabel(gear) {
  if (gear === 250) return "R";
  if (gear === 251) return "N";
  return String(gear);
}

// --- Controls hint and gear hints (reverse discoverability) -------------------
// Keys are matched by event.code, so the physical A/Z positions work on every
// keyboard layout.
export const CONTROLS_HINT = "Arrows: drive · Space: handbrake · Z/A: shift down/up · Z Z at standstill: reverse (A A: forward) · R: reset";
/** Hint for the manual gearbox: A/Z step through R, N, 1-6 one gear per tap. */
export const CONTROLS_HINT_MANUAL = "Arrows: drive · Space: handbrake · Manual gearbox: A shifts up, Z shifts down (R, N, 1, 2, 3 ...) · start in N, press A for 1st · R: reset";

/** Auto reverse on: no shifting; the brake reverses at a standstill. */
export const CONTROLS_HINT_AUTO_REVERSE = "Arrows: drive · Up: accelerate · Down: brake, hold at standstill to reverse (gear shows R) · Up in R: brake, then drive forward · Space: handbrake · R: reset";
export const CONTROLS_HINT_AUTO_REVERSE_MANUAL = "Arrows: drive · Down at standstill: reverse (R), Up in R: back to 1st · Manual gearbox: A shifts up, Z shifts down · Space: handbrake · R: reset";

/** Controls hint for the chosen gearbox mode ("manual" or anything else for automatic). */
export function controlsHintFor(transmission, autoReverse = false) {
  if (autoReverse) return transmission === "manual" ? CONTROLS_HINT_AUTO_REVERSE_MANUAL : CONTROLS_HINT_AUTO_REVERSE;
  return transmission === "manual" ? CONTROLS_HINT_MANUAL : CONTROLS_HINT;
}

export const GEAR_HINT_NEUTRAL_THROTTLE = "Press A to select 1st";
export const GEAR_HINT_REVERSE_FROM_FORWARD = "Press Z twice to reverse";
export const GEAR_HINT_REVERSE_FROM_NEUTRAL = "Press Z to select reverse";

const GEAR_R = 250;
const GEAR_N = 251;

/**
 * Tracks how long the driver has held accelerate in neutral (>1 s: "Press A
 * to select 1st") or the brake at standstill in a forward gear or neutral
 * (>1.5 s: how to reverse). With `autoReverse` the simulation handles
 * reversing by itself, so no hint is shown.
 * `update` returns `{ hint }`; call it once per fixed step.
 */
export function createGearHintTracker({
  neutralThrottleSeconds = 1,
  brakeSeconds = 1.5,
  standstillMps = 0.5,
} = {}) {
  let neutralHeld = 0;
  let brakeHeld = 0;
  return {
    reset() { neutralHeld = 0; brakeHeld = 0; },
    update({ dt, live, gear, speed, throttle, brake, autoReverse = false }) {
      const none = { hint: null };
      if (!live || !Number.isFinite(dt) || dt < 0 || !Number.isInteger(gear) || gear <= 0) {
        this.reset();
        return none;
      }
      const standing = Math.abs(Number.isFinite(speed) ? speed : 0) < standstillMps;
      neutralHeld = gear === GEAR_N && throttle > 0.2 ? neutralHeld + dt : 0;
      const canReverse = gear !== GEAR_R && standing && brake > 0.5 && !(throttle > 0.2);
      brakeHeld = canReverse ? brakeHeld + dt : 0;
      let hint = null;
      if (neutralHeld > neutralThrottleSeconds) hint = GEAR_HINT_NEUTRAL_THROTTLE;
      else if (brakeHeld > brakeSeconds) hint = gear === GEAR_N ? GEAR_HINT_REVERSE_FROM_NEUTRAL : GEAR_HINT_REVERSE_FROM_FORWARD;
      if (autoReverse) hint = null;
      return { hint };
    },
  };
}

/** CSS-friendly gear state for the indicator: "reverse", "neutral", "forward" or "none". */
export function gearKind(gear) {
  if (gear === GEAR_R) return "reverse";
  if (gear === GEAR_N) return "neutral";
  return Number.isInteger(gear) && gear > 0 && gear < GEAR_R ? "forward" : "none";
}

export function buildHudModel({
  connected,
  localPracticeMode,
  openDrive,
  snapshot,
  ownVehicle,
  networkStatus,
  inRoom,
  raceSummary,
  elapsedTicks,
  localLapTarget = 2,
  speedUnit = "kmh",
  vehicles = null,
  localCompetitors = 1,
}) {
  const status = statusKind(networkStatus ?? raceSummary?.status);
  const state = connected
    ? (status || (inRoom ? "joining" : "lobby"))
    : localPracticeMode
      ? (openDrive ? "open drive" : "practice")
      : "local";
  const speed = connected ? ownVehicle?.speed : snapshot?.speed;
  const gear = connected ? ownVehicle?.gear : snapshot?.gear;
  const hasGearbox = Number.isInteger(gear) && gear > 0;
  const converted = convertSpeed(speed, speedUnit, { metersPerSecond: hasGearbox });
  const displayedSpeed = converted === null ? "—" : converted.toFixed(1);
  let lap = null;
  if (connected && (status === "racing" || status === "finished") && ownVehicle
    && Number.isInteger(ownVehicle.completed_laps)
    && Number.isInteger(raceSummary?.laps) && raceSummary.laps > 0) {
    lap = { current: Math.min(ownVehicle.completed_laps + 1, raceSummary.laps), target: raceSummary.laps };
  } else if (!connected && !openDrive && Number.isInteger(snapshot?.lap)
    && Number.isInteger(localLapTarget) && localLapTarget > 0) {
    lap = { current: Math.min(snapshot.lap, localLapTarget), target: localLapTarget };
  }
  let race = null;
  if (connected && (status === "racing" || status === "finished") && ownVehicle) {
    race = buildRaceTimingModel(ownVehicle, Array.isArray(vehicles) ? vehicles.length : 0);
  } else if (!connected && !openDrive && snapshot) {
    race = buildRaceTimingModel(snapshot, Number.isInteger(localCompetitors) && localCompetitors > 0 ? localCompetitors : 1);
  }
  // Diagnostics only: which vehicle model drives the own car right now
  // (local snapshot, or the server's per-vehicle model id when connected).
  const handling = handlingLabel(connected ? ownVehicle?.handling_model : snapshot?.handling_model);
  return {
    speed: displayedSpeed,
    speedUnitLabel: SPEED_UNIT_LABELS[speedUnit] ?? SPEED_UNIT_LABELS.kmh,
    ...(hasGearbox ? { gear: gearLabel(gear) } : {}),
    ...(handling ? { handling } : {}),
    session: formatSessionTime(elapsedTicks),
    state: state.toUpperCase(),
    lap,
    race,
  };
}

const MAX_ROUTE_MAP_POINTS = 65_535;
const MAX_ROUTE_MAP_COORDINATE = 1_000_000;

// Keep this as a rendered path only. It does not imply road width, checkpoints,
// progress, or a race route contract.
export function buildRouteMapModel(path) {
  if (!Array.isArray(path) || path.length < 2 || path.length > MAX_ROUTE_MAP_POINTS) return null;
  const points = new Float32Array(path.length * 2);
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (let index = 0; index < path.length; index++) {
    const point = path[index];
    if (!Array.isArray(point) || point.length !== 3
      || point.some((value) => !Number.isFinite(value) || Math.abs(value) > MAX_ROUTE_MAP_COORDINATE)) return null;
    const x = point[0], y = point[1];
    points[index * 2] = x;
    points[index * 2 + 1] = y;
    minX = Math.min(minX, x); minY = Math.min(minY, y);
    maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
  }
  const padDegenerate = (min, max) => {
    if (max - min > 1e-6) return [min, max];
    const padding = Math.max(1, Math.abs(min) * 0.01);
    return [min - padding, max + padding];
  };
  [minX, maxX] = padDegenerate(minX, maxX);
  [minY, maxY] = padDegenerate(minY, maxY);
  return { points, minX, minY, maxX, maxY };
}

export function fitRouteMap(model, width, height, padding = 8) {
  if (!model?.points || !Number.isInteger(width) || !Number.isInteger(height)
    || width < 1 || height < 1 || width > 1024 || height > 1024
    || !Number.isFinite(padding) || padding < 0) return null;
  const innerWidth = width - 2 * padding, innerHeight = height - 2 * padding;
  if (innerWidth <= 0 || innerHeight <= 0) return null;
  const worldWidth = model.maxX - model.minX, worldHeight = model.maxY - model.minY;
  if (!(worldWidth > 0) || !(worldHeight > 0) || !Number.isFinite(worldWidth + worldHeight)) return null;
  const scale = Math.min(innerWidth / worldWidth, innerHeight / worldHeight);
  const drawWidth = worldWidth * scale, drawHeight = worldHeight * scale;
  const offsetX = (width - drawWidth) / 2;
  const offsetY = (height - drawHeight) / 2;
  const screenPoints = new Float32Array(model.points.length);
  for (let index = 0; index < model.points.length; index += 2) {
    screenPoints[index] = offsetX + (model.points[index] - model.minX) * scale;
    screenPoints[index + 1] = offsetY + (model.maxY - model.points[index + 1]) * scale;
  }
  return { points: screenPoints, width, height, scale, offsetX, offsetY, minX: model.minX, minY: model.minY, maxX: model.maxX, maxY: model.maxY };
}

export function projectRouteMapMarker(fit, x, y, heading, length = 9) {
  if (!fit || ![x, y, heading, length].every(Number.isFinite) || length <= 0) return null;
  const markerX = fit.offsetX + (x - fit.minX) * fit.scale;
  const markerY = fit.offsetY + (fit.maxY - y) * fit.scale;
  const dx = Math.cos(heading), dy = -Math.sin(heading);
  return {
    x: markerX,
    y: markerY,
    tipX: markerX + dx * length,
    tipY: markerY + dy * length,
    inBounds: x >= fit.minX && x <= fit.maxX && y >= fit.minY && y <= fit.maxY
      && markerX >= 0 && markerX <= fit.width && markerY >= 0 && markerY <= fit.height,
  };
}

export function createRouteMapView({ panel, canvas, game } = {}) {
  const context = canvas?.getContext("2d");
  let model = null, fit = null, staticCanvas = null, rasterKey = null;
  const setPath = (path) => {
    model = buildRouteMapModel(path);
    fit = null; staticCanvas = null; rasterKey = null;
    const available = !!model && !!context;
    if (panel) panel.hidden = !available;
    game?.classList.toggle("has-route-map", available);
    return available;
  };
  const prepare = () => {
    if (!model || !context || !canvas) return false;
    const rect = canvas.getBoundingClientRect();
    if (!(rect.width > 0) || !(rect.height > 0)) return false;
    const dpr = Math.min(globalThis.window?.devicePixelRatio || 1, 2);
    const width = Math.max(1, Math.min(512, Math.round(rect.width * dpr)));
    const height = Math.max(1, Math.min(512, Math.round(rect.height * dpr)));
    const key = `${width}x${height}`;
    if (key === rasterKey && fit && staticCanvas) return true;
    canvas.width = width; canvas.height = height;
    fit = fitRouteMap(model, width, height, Math.max(4, Math.round(8 * dpr)));
    staticCanvas = globalThis.document?.createElement("canvas") ?? null;
    if (!fit || !staticCanvas) { fit = null; staticCanvas = null; return false; }
    staticCanvas.width = width; staticCanvas.height = height;
    const staticContext = staticCanvas.getContext("2d");
    if (!staticContext) { fit = null; staticCanvas = null; return false; }
    staticContext.beginPath();
    for (let index = 0; index < fit.points.length; index += 2) {
      const x = fit.points[index], y = fit.points[index + 1];
      index === 0 ? staticContext.moveTo(x, y) : staticContext.lineTo(x, y);
    }
    staticContext.closePath();
    staticContext.lineCap = "round"; staticContext.lineJoin = "round";
    staticContext.strokeStyle = "#dce5d49c";
    staticContext.lineWidth = Math.max(1, 1.5 * dpr);
    staticContext.stroke();
    rasterKey = key;
    return true;
  };
  const draw = (position = null) => {
    if (!prepare()) return false;
    const width = canvas.width, height = canvas.height;
    context.clearRect(0, 0, width, height);
    context.drawImage(staticCanvas, 0, 0);
    if (![position?.x, position?.y, position?.heading].every(Number.isFinite)) return true;
    const marker = projectRouteMapMarker(fit, position.x, position.y, position.heading, Math.max(7, Math.min(width, height) * 0.08));
    if (!marker?.inBounds) return true;
    const dpr = Math.min(globalThis.window?.devicePixelRatio || 1, 2);
    context.beginPath(); context.moveTo(marker.x, marker.y); context.lineTo(marker.tipX, marker.tipY);
    context.strokeStyle = "#d3f36b"; context.lineWidth = Math.max(2, dpr * 2); context.lineCap = "round"; context.stroke();
    context.beginPath(); context.arc(marker.x, marker.y, Math.max(2, dpr * 2), 0, Math.PI * 2);
    context.fillStyle = "#d3f36b"; context.fill();
    return true;
  };
  // Position inside the drawn map as fractions of its canvas (for markers drawn elsewhere), or null.
  const locate = (position) => {
    if (!prepare() || ![position?.x, position?.y].every(Number.isFinite)) return null;
    const marker = projectRouteMapMarker(fit, position.x, position.y, 0, 1);
    return marker?.inBounds ? { u: marker.x / fit.width, v: marker.y / fit.height } : null;
  };
  return { setPath, draw, locate, available: () => !!model && !!context };
}

export function setRouteMapFromSceneDescriptor(view, descriptor) {
  return view?.setPath(descriptor?.preview_reference_path_scene ?? null) ?? false;
}
