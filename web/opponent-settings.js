// Computer-opponent settings shared by the solo and room set-up UI, plus the
// pure helpers that turn local opponent state into render poses and audio
// voices. No DOM here; unit tested.

export const DEFAULT_OPPONENTS = 7;
export const MAX_OPPONENTS = 7;
export const SKILL_BEGINNER = 0;
export const SKILL_ADVANCED = 1;
const FLOATS_PER_OPPONENT = 10;

export function clampOpponents(value, fallback = DEFAULT_OPPONENTS) {
  const number = typeof value === "string" && value.trim() === "" ? NaN : Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.min(MAX_OPPONENTS, Math.max(0, Math.trunc(number)));
}

/** Laps of a race on an installed track: 0 is free driving, otherwise 1..5. */
export const DEFAULT_IMPORTED_LAPS = 1;
export const MAX_IMPORTED_LAPS = 5;
export function clampLaps(value, fallback = DEFAULT_IMPORTED_LAPS) {
  const number = typeof value === "string" && value.trim() === "" ? NaN : Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.min(MAX_IMPORTED_LAPS, Math.max(0, Math.trunc(number)));
}

export function clampSkill(value) {
  return Number(value) === SKILL_BEGINNER ? SKILL_BEGINNER : SKILL_ADVANCED;
}

/**
 * Solo settings from a query string. A missing `ai` parameter means no
 * opponents, so existing practice URLs keep their meaning; the set-up UI
 * always writes the parameter.
 */
export function parseOpponentParams(search) {
  const params = new URLSearchParams(search);
  return {
    count: params.has("ai") ? clampOpponents(params.get("ai"), 0) : 0,
    skill: params.has("skill") ? clampSkill(params.get("skill")) : SKILL_ADVANCED,
    catchUp: params.get("catchup") !== "0",
    laps: params.has("laps") ? clampLaps(params.get("laps")) : DEFAULT_IMPORTED_LAPS,
  };
}

export function soloQuery({ count, skill, catchUp, track, transmission, laps }) {
  const gearbox = transmission === "manual" ? "&gearbox=manual" : "";
  const trackPart = typeof track === "string" && /^[a-z0-9][a-z0-9-]{0,47}$/.test(track) ? `&track=${track}` : "";
  // The default (one lap) stays out of the link so existing links keep their shape.
  const lapsPart = laps === undefined || laps === null || clampLaps(laps) === DEFAULT_IMPORTED_LAPS ? "" : `&laps=${clampLaps(laps)}`;
  return `practice=local&ai=${clampOpponents(count)}&skill=${clampSkill(skill)}&catchup=${catchUp === false ? 0 : 1}${lapsPart}${gearbox}${trackPart}`;
}

export function skillLabel(skill) {
  return clampSkill(skill) === SKILL_BEGINNER ? "Beginner" : "Advanced";
}

export function opponentSummary({ count, skill, catchUp }) {
  const n = clampOpponents(count, 0);
  if (n === 0) return "No computer opponents";
  return `${n} computer opponent${n === 1 ? "" : "s"} · ${skillLabel(skill)} · catch-up ${catchUp === false ? "off" : "on"}`;
}

/** Decode the flat `opponent_states` array (10 numbers per opponent). */
export function decodeOpponentStates(flat) {
  const out = [];
  if (!flat || typeof flat.length !== "number") return out;
  for (let i = 0; i + FLOATS_PER_OPPONENT <= flat.length; i += FLOATS_PER_OPPONENT) {
    out.push({
      id: flat[i], x: flat[i + 1], y: flat[i + 2], heading: flat[i + 3],
      vx: flat[i + 4], vy: flat[i + 5], speed: flat[i + 6], laps: flat[i + 7],
      finished: flat[i + 8] === 1, position: flat[i + 9],
    });
  }
  return out;
}

/** Render roster signature plus x,y,heading triples with the own car first. */
export function projectRenderPoses(own, opponents) {
  const cars = [own, ...(opponents ?? [])];
  const poses = new Float32Array(cars.length * 3);
  cars.forEach((car, index) => poses.set([car.x, car.y, car.heading], index * 3));
  return { poses, signature: `project-circuit:${cars.length}`, localIndex: 0 };
}

/** Vehicle-like rows for the remote-voice feed. */
export function opponentVoices(opponents) {
  return (opponents ?? []).map((car) => ({
    player_id: car.id,
    x: car.x,
    y: car.y,
    vehicle_profile_id: "vehicle-profile-project-default",
    rpm_q8: Math.max(0, Math.min(255, Math.round(60 + car.speed * 7))),
    gear: 0,
  }));
}
