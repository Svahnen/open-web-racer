const SPEED_UNITS = new Set(["kmh", "mph", "raw"]);

export function readSpeedUnit(search) {
  const requested = new URLSearchParams(search).get("speedUnit");
  return SPEED_UNITS.has(requested) ? requested : "kmh";
}

export const DEFAULT_SOUND_VOLUME = 0.7;

export function clampVolume(value, fallback = DEFAULT_SOUND_VOLUME) {
  const number = value === null || value === undefined || (typeof value === "string" && value.trim() === "") ? NaN : Number(value);
  return Number.isFinite(number) ? Math.min(1, Math.max(0, number)) : fallback;
}

// Sound preference lives in the query string like the speed unit: ?sound=off&volume=0.5
export function readSoundPreference(search) {
  const params = new URLSearchParams(search);
  return { muted: params.get("sound") === "off", volume: clampVolume(params.get("volume")) };
}

export function soundPreferenceLocation(href, { muted, volume }) {
  const url = new URL(href);
  if (muted) url.searchParams.set("sound", "off"); else url.searchParams.delete("sound");
  url.searchParams.set("volume", String(Math.round(clampVolume(volume) * 100) / 100));
  return `${url.pathname}${url.search}${url.hash}`;
}

export function speedUnitPreferenceLocation(href, unit) {
  const url = new URL(href);
  url.searchParams.set("speedUnit", SPEED_UNITS.has(unit) ? unit : "kmh");
  return `${url.pathname}${url.search}${url.hash}`;
}
