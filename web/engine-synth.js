// Generic additive engine synthesiser driven by the `engine_sound` block of an
// RCP vehicle entry (docs/CONTENT_PACK.md, "EngineSynthParams"). Project-owned
// and parametric: the firing frequency follows engine speed and the cylinder
// count, a few harmonics are mixed at fixed relative levels, and a low-pass
// filter opens with engine speed. Used for project cars when no owner audio is
// loaded.

export const MAX_HARMONICS = 8;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const finite = (v) => typeof v === "number" && Number.isFinite(v);
const lerp = (a, b, t) => a + (b - a) * t;

/** Validate a raw `engine_sound` object; returns a normalised copy or null. */
export function parseEngineSound(raw) {
  if (!raw || typeof raw !== "object") return null;
  const { idle_rpm, redline_rpm, cylinders, harmonics, filter_hz_idle, filter_hz_redline, noise_gain, gain_idle, gain_redline } = raw;
  if (![idle_rpm, redline_rpm, filter_hz_idle, filter_hz_redline, noise_gain, gain_idle, gain_redline].every(finite)) return null;
  if (idle_rpm <= 0 || redline_rpm <= idle_rpm) return null;
  if (!Number.isInteger(cylinders) || cylinders < 1 || cylinders > 16) return null;
  if (!Array.isArray(harmonics) || harmonics.length < 1 || harmonics.length > MAX_HARMONICS || !harmonics.every(finite)) return null;
  return {
    idleRpm: idle_rpm,
    redlineRpm: redline_rpm,
    cylinders,
    harmonics: harmonics.map((h) => clamp(h, 0, 1)),
    filterHzIdle: clamp(filter_hz_idle, 40, 20000),
    filterHzRedline: clamp(filter_hz_redline, 40, 20000),
    noiseGain: clamp(noise_gain, 0, 1),
    gainIdle: clamp(gain_idle, 0, 1),
    gainRedline: clamp(gain_redline, 0, 1),
  };
}

/**
 * Voice parameters for one frame. `rpm01` is engine speed as a fraction of the
 * redline (the simulation's telemetry), `throttle` is 0..1. Pure.
 */
export function engineSynthParams(sound, { rpm01 = 0, throttle = 0 } = {}, nyquistHz = 20000) {
  const load = clamp(finite(throttle) ? throttle : 0, 0, 1);
  const rpm = clamp((finite(rpm01) ? rpm01 : 0) * sound.redlineRpm, sound.idleRpm, sound.redlineRpm);
  const x = (rpm - sound.idleRpm) / (sound.redlineRpm - sound.idleRpm);
  const fundamentalHz = (rpm / 60) * (sound.cylinders / 2);
  const limit = Math.min(nyquistHz * 0.45, 12000);
  const harmonicGains = sound.harmonics.map((level, i) => ((i + 1) * fundamentalHz <= limit ? level : 0));
  const total = harmonicGains.reduce((a, b) => a + b, 0) || 1;
  const level = lerp(sound.gainIdle, sound.gainRedline, x) * (0.65 + 0.35 * load);
  return {
    rpm,
    x,
    fundamentalHz,
    harmonicGains: harmonicGains.map((g) => (g / total) * level),
    filterHz: lerp(sound.filterHzIdle, sound.filterHzRedline, x) * (0.8 + 0.4 * load),
    noiseGain: sound.noiseGain * (0.4 + 0.6 * load) * (0.5 + 0.5 * x),
  };
}

/** Web Audio voice built from `engineSynthParams`. */
export class EngineSynthVoice {
  constructor({ ctx, destination, noiseBuffer = null, sound }) {
    this.ctx = ctx;
    this.sound = sound;
    this.sources = [];
    this.filter = ctx.createBiquadFilter();
    this.filter.type = "lowpass";
    this.out = ctx.createGain();
    this.out.gain.value = 0;
    this.filter.connect(this.out);
    this.out.connect(destination);
    this.oscillators = sound.harmonics.map(() => {
      const osc = ctx.createOscillator();
      osc.type = "sine";
      const gain = ctx.createGain();
      gain.gain.value = 0;
      osc.connect(gain);
      gain.connect(this.filter);
      osc.start();
      this.sources.push(osc);
      return { osc, gain };
    });
    this.noiseGain = ctx.createGain();
    this.noiseGain.gain.value = 0;
    if (noiseBuffer) {
      const noise = ctx.createBufferSource();
      noise.buffer = noiseBuffer;
      noise.loop = true;
      noise.connect(this.noiseGain);
      this.noiseGain.connect(this.filter);
      noise.start();
      this.sources.push(noise);
    }
  }

  update(state, tc = 0.05) {
    const t = this.ctx.currentTime;
    const p = engineSynthParams(this.sound, state, this.ctx.sampleRate / 2 || 22050);
    this.oscillators.forEach(({ osc, gain }, i) => {
      osc.frequency.setTargetAtTime(p.fundamentalHz * (i + 1), t, tc);
      gain.gain.setTargetAtTime(p.harmonicGains[i], t, tc);
    });
    this.filter.frequency.setTargetAtTime(p.filterHz, t, tc);
    this.noiseGain.gain.setTargetAtTime(p.noiseGain, t, tc);
    this.out.gain.setTargetAtTime(1, t, tc);
    return p;
  }

  silence() {
    this.out.gain.setTargetAtTime(0, this.ctx.currentTime, 0.05);
  }

  dispose() {
    for (const s of this.sources) {
      try { s.stop(); } catch { /* already stopped */ }
      try { s.disconnect(); } catch { /* ignore */ }
    }
    this.sources = [];
    try { this.out.disconnect(); } catch { /* ignore */ }
  }
}
