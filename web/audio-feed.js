// Pure helpers that turn client snapshots into inputs for sampled voices
// provided by an audio plugin (docs/PLUGINS.md). No Web Audio here; unit tested.

/**
 * Remote cars as voices: nearest listener-relative distance and the sideways
 * offset (positive = to the right of the listener's heading).
 */
export function remoteCarsForAudio(vehicles, selfPlayerId, self, stemOf) {
  if (!Array.isArray(vehicles) || !self || !Number.isFinite(self.x) || !Number.isFinite(self.y)) return [];
  const sin = Math.sin(self.heading ?? 0), cos = Math.cos(self.heading ?? 0);
  const out = [];
  for (const vehicle of vehicles) {
    if (!vehicle || vehicle.player_id === selfPlayerId || !Number.isFinite(vehicle.x) || !Number.isFinite(vehicle.y)) continue;
    const dx = vehicle.x - self.x, dy = vehicle.y - self.y;
    out.push({
      id: vehicle.player_id,
      stem: stemOf?.(vehicle.vehicle_profile_id) ?? null,
      dist: Math.hypot(dx, dy),
      relRight: dx * sin - dy * cos,
      rpm01: Number.isFinite(vehicle.rpm_q8) && vehicle.gear !== 255 ? vehicle.rpm_q8 / 255 : 0.25,
    });
  }
  return out;
}

/**
 * Project heuristic until the client exposes contact events: a speed loss
 * faster than any braking (measured braking is about 42 m/s^2) is a hit.
 * Returns the spec's Q16 impact magnitude (metres per second lost, Q16) or 0.
 */
export const IMPACT_MIN_DECEL = 80;
export const IMPACT_MIN_SPEED_LOSS = 2;
export const IMPACT_COOLDOWN_S = 0.25;

export class ImpactDetector {
  constructor() { this.reset(); }

  reset(suppressSeconds = 0.5) {
    this.previousSpeed = null;
    this.cooldown = suppressSeconds;
  }

  /** Feed the local speed (m/s) once per frame; returns { magnitudeQ16, lateralQ16 } on a hit, else null. */
  update(speed, dt, lateralSpeed = 0) {
    if (!Number.isFinite(speed) || !(dt > 0)) return null;
    this.cooldown = Math.max(0, this.cooldown - dt);
    const previous = this.previousSpeed;
    this.previousSpeed = speed;
    if (previous === null || this.cooldown > 0) return null;
    const loss = previous - speed;
    if (loss < IMPACT_MIN_SPEED_LOSS || loss / dt < IMPACT_MIN_DECEL) return null;
    this.cooldown = IMPACT_COOLDOWN_S;
    return { magnitudeQ16: Math.round(loss * 65536), lateralQ16: Math.round((Number.isFinite(lateralSpeed) ? lateralSpeed : 0) * 65536) };
  }
}
