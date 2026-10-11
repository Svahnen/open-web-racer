// Project-owned synthesized audio (Web Audio only; no sampled assets) is the
// default. Placeholder engine/skid/thump voices; setEngineState({ rpm01 }) is
// driven by the RPM/gear model. When an audio plugin supplies a sample library
// (docs/PLUGINS.md), setSampleLibrary() swaps in its voices; the synthesized
// voices stay as the fallback and are silenced only while the sampled voices
// are active.

import { EngineSynthVoice, parseEngineSound } from "./engine-synth.js";
import { RacerPlugins } from "./plugins.js";

export const MAX_AUDIO_SPEED = 60; // simulation units/s mapped to full engine pitch

const clamp01 = (v) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0);

/** Map engine state to synth parameters. Pure; unit tested. */
export function engineParams({ rpm01, throttle = 0, speed = 0 } = {}) {
  const rpm = rpm01 === undefined || rpm01 === null
    ? clamp01(Math.abs(speed) / MAX_AUDIO_SPEED) ** 0.8
    : clamp01(rpm01);
  const load = clamp01(throttle);
  return {
    rpm,
    baseHz: 45 + rpm * 190,
    detuneCents: 12 + rpm * 8,
    gain: 0.12 + rpm * 0.2 + load * 0.2,
    noiseGain: 0.03 + load * 0.08 + rpm * 0.05,
    filterHz: 350 + rpm * 1400 + load * 900,
  };
}

/** Skid loudness (0..1) from lateral speed; quiet below threshold or when nearly stopped. */
export function skidLevel(lateral, speed = 0, threshold = 4) {
  if (!Number.isFinite(lateral) || !Number.isFinite(speed) || Math.abs(speed) < 5) return 0;
  return clamp01((Math.abs(lateral) - threshold) / 10);
}

/** Lateral velocity component from world velocity and heading (CCW, XY plane). NaN if unknown. */
export function lateralSpeed(vx, vy, heading) {
  if (![vx, vy, heading].every(Number.isFinite)) return NaN;
  return -vx * Math.sin(heading) + vy * Math.cos(heading);
}

export class GameAudio {
  constructor({ AudioContextClass = globalThis.AudioContext ?? globalThis.webkitAudioContext, volume = 0.7, muted = false } = {}) {
    this.AudioContextClass = AudioContextClass;
    this.volume = clamp01(volume);
    this.muted = !!muted;
    this.ctx = null;
    this.nodes = null;
    this.library = null;
    this.carSpec = null;
    this.sampled = null;
    this.musicProvider = null;
    this.musicDispose = null;
    this.engineSound = null; // parsed `engine_sound` of the current project car
    this.synthVoice = null;
  }

  /**
   * Parametric engine voice of a project car (RCP `engine_sound`); null or an
   * invalid block returns to the placeholder tone. Sampled sounds take priority.
   */
  setEngineSound(raw) {
    this.engineSound = parseEngineSound(raw);
    this.#rebuildSynth();
  }

  #rebuildSynth() {
    this.synthVoice?.dispose();
    this.synthVoice = null;
    if (!this.nodes || !this.engineSound) return;
    try {
      this.synthVoice = new EngineSynthVoice({ ctx: this.ctx, destination: this.nodes.buses.engine, noiseBuffer: this.nodes.noiseBuffer, sound: this.engineSound });
    } catch {
      this.synthVoice = null;
    }
  }

  /**
   * Use plugin-supplied sampled sounds. `library` is whatever the audio plugin discovered
   * (or null to go back to synthesized sounds); `carSpec` = { stem, ceiling }.
   * Safe to call before unlock(); the voices are built once audio exists.
   */
  setSampleLibrary(library, carSpec = null) {
    this.#teardownSampled();
    this.library = library ?? null;
    this.carSpec = carSpec;
    this.#ensureSampled();
  }

  /**
   * Music hook. The install contains no music data (it is played from the disc
   * as CD audio), so nothing is attached by default and no music plays. A
   * future provider `({ ctx, master }) => dispose|void` is invoked whenever the
   * audio graph is (re)built.
   */
  setMusicProvider(provider) {
    this.musicProvider = typeof provider === "function" ? provider : null;
    this.#attachMusic();
  }

  #attachMusic() {
    this.musicDispose?.();
    this.musicDispose = null;
    if (!this.musicProvider || !this.nodes) return;
    try { const dispose = this.musicProvider({ ctx: this.ctx, master: this.nodes.master }); if (typeof dispose === "function") this.musicDispose = dispose; } catch { /* music is optional */ }
  }

  /** True while sampled sounds (not the synthesized placeholder) are audible. */
  get sampledActive() { return !!this.sampled?.active; }

  #ensureSampled() {
    if (!this.library || !this.nodes || this.sampled) return;
    try {
      const voices = RacerPlugins.invoke("audio", "createVoices", [{ ctx: this.ctx, library: this.library, buses: this.nodes.buses }], null);
      if (voices && voices.setCar(this.carSpec ?? {})) this.sampled = voices;
      else voices?.dispose();
    } catch {
      this.sampled = null;
    }
    if (this.sampled) this.#muteSynth();
  }

  #teardownSampled() {
    this.sampled?.dispose();
    this.sampled = null;
  }

  #muteSynth() {
    const n = this.nodes;
    if (!n) return;
    const t = this.ctx.currentTime;
    n.toneGain.gain.setTargetAtTime(0, t, 0.02);
    n.engineNoiseGain.gain.setTargetAtTime(0, t, 0.02);
    n.skidGain.gain.setTargetAtTime(0, t, 0.02);
    this.synthVoice?.silence();
  }

  get supported() { return typeof this.AudioContextClass === "function"; }

  /** Call from a user gesture handler (keydown/pointerdown/click). Safe to repeat. */
  unlock() {
    if (!this.supported) return false;
    if (!this.ctx) {
      try { this.ctx = new this.AudioContextClass(); } catch { this.ctx = null; return false; }
      this.#build();
    }
    if (this.ctx.state === "suspended") this.ctx.resume?.()?.catch?.(() => {});
    return true;
  }

  #build() {
    const ctx = this.ctx;
    const master = ctx.createGain();
    master.connect(ctx.destination);
    const bus = () => { const g = ctx.createGain(); g.connect(master); return g; };
    const buses = { engine: bus(), effects: bus(), ui: bus() };
    const osc = (type) => { const o = ctx.createOscillator(); o.type = type; return o; };
    const oscA = osc("sawtooth"), oscB = osc("square");
    const engineFilter = ctx.createBiquadFilter();
    engineFilter.type = "lowpass";
    const toneGain = ctx.createGain();
    oscA.connect(engineFilter); oscB.connect(engineFilter);
    engineFilter.connect(toneGain); toneGain.connect(buses.engine);
    // Shared looping noise buffer (deterministic LCG) for engine roughness and skid.
    const rate = ctx.sampleRate || 44100;
    const buffer = ctx.createBuffer(1, rate, rate);
    const data = buffer.getChannelData(0);
    let seed = 1;
    for (let i = 0; i < data.length; i++) { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; data[i] = seed / 2147483648 - 1; }
    const noiseSource = () => { const n = ctx.createBufferSource(); n.buffer = buffer; n.loop = true; return n; };
    const engineNoise = noiseSource(), engineNoiseGain = ctx.createGain();
    engineNoise.connect(engineNoiseGain); engineNoiseGain.connect(engineFilter);
    const skidNoise = noiseSource(), skidFilter = ctx.createBiquadFilter(), skidGain = ctx.createGain();
    skidFilter.type = "bandpass"; skidFilter.frequency.value = 1800; skidFilter.Q.value = 1.5;
    skidNoise.connect(skidFilter); skidFilter.connect(skidGain); skidGain.connect(buses.effects);
    toneGain.gain.value = 0; engineNoiseGain.gain.value = 0; skidGain.gain.value = 0;
    const sources = [oscA, oscB, engineNoise, skidNoise];
    for (const s of sources) s.start();
    this.nodes = { master, buses, oscA, oscB, engineFilter, toneGain, engineNoiseGain, skidGain, sources, noiseBuffer: buffer };
    this.#rebuildSynth();
    this.#applyMaster();
    this.#ensureSampled();
    this.#attachMusic();
  }

  #applyMaster() {
    if (this.nodes) this.nodes.master.gain.value = this.muted ? 0 : this.volume;
  }

  setVolume(volume) { this.volume = clamp01(volume); this.#applyMaster(); }
  setMuted(muted) { this.muted = !!muted; this.#applyMaster(); }

  /** { rpm01?, throttle?, speed? } — rpm01 overrides the speed-derived estimate. */
  setEngineState(state, dt) {
    if (!this.nodes) return;
    if (this.sampledActive) {
      this.#muteSynth();
      this.sampled.updateEngine({ ...state, rpm01: engineParams(state).rpm }, dt);
      return;
    }
    const p = engineParams(state);
    const t = this.ctx.currentTime, tc = 0.06; // smoothing time constant
    const n = this.nodes;
    if (this.synthVoice) {
      n.toneGain.gain.setTargetAtTime(0, t, tc);
      n.engineNoiseGain.gain.setTargetAtTime(0, t, tc);
      this.synthVoice.update({ rpm01: p.rpm, throttle: state?.throttle ?? 0 }, tc);
      return;
    }
    n.oscA.frequency.setTargetAtTime(p.baseHz, t, tc);
    n.oscB.frequency.setTargetAtTime(p.baseHz, t, tc);
    n.oscB.detune.setTargetAtTime(p.detuneCents, t, tc);
    n.engineFilter.frequency.setTargetAtTime(p.filterHz, t, tc);
    n.toneGain.gain.setTargetAtTime(p.gain, t, tc);
    n.engineNoiseGain.gain.setTargetAtTime(p.noiseGain, t, tc);
  }

  /** Sampled tyre squeal from lateral slip speed (m/s); no-op for the synthesized fallback. */
  setTyreSlip({ lateralMps, speed = 0, racing = true } = {}, dt) {
    if (this.sampledActive) this.sampled.updateSkid({ lateralMps, speed, racing }, dt);
  }

  /** Horn button state (key H). Sampled sounds only. */
  setHorn(held) {
    if (this.sampledActive) this.sampled.setHorn(!!held);
  }

  /** Remote cars: [{ id, stem, dist, relRight, rpm01, ceiling }]. Sampled sounds only. */
  setRemoteCars(list, dt) {
    if (this.sampledActive) this.sampled.updateRemotes(list, dt);
  }

  /**
   * Collision one-shot. `event` = { magnitudeQ16, lateralQ16 } (spec section 5);
   * falls back to the synthesized thump scaled by `strength`.
   */
  playImpact(event, strength = 1) {
    if (this.sampledActive && this.sampled.playImpact(event)) return;
    this.playThump(strength);
  }

  setSkid(level) {
    if (this.sampledActive) { this.#muteSynth(); return; }
    if (this.nodes) this.nodes.skidGain.gain.setTargetAtTime(clamp01(level) * 0.25, this.ctx.currentTime, 0.05);
  }

  /** Short low thump. TODO: call from contact events once the client exposes them. */
  playThump(strength = 1) {
    if (!this.nodes) return;
    const ctx = this.ctx, t = ctx.currentTime, s = clamp01(strength);
    const o = ctx.createOscillator(), g = ctx.createGain();
    o.type = "sine";
    o.frequency.setValueAtTime(110, t);
    o.frequency.exponentialRampToValueAtTime(40, t + 0.18);
    g.gain.setValueAtTime(0.6 * s + 0.001, t);
    g.gain.exponentialRampToValueAtTime(0.001, t + 0.22);
    o.connect(g); g.connect(this.nodes.buses.effects);
    o.start(t); o.stop(t + 0.25);
  }

  /** Fade engine/skid to silence without tearing down (idle between races). */
  silence() {
    if (!this.nodes) return;
    this.sampled?.silence();
    this.synthVoice?.silence();
    this.setSkid(0);
    const t = this.ctx.currentTime;
    this.nodes.toneGain.gain.setTargetAtTime(0, t, 0.05);
    this.nodes.engineNoiseGain.gain.setTargetAtTime(0, t, 0.05);
  }

  /** Stop all sources and close the context. A later unlock() builds a fresh graph. */
  dispose() {
    const n = this.nodes, ctx = this.ctx;
    this.musicDispose?.(); this.musicDispose = null;
    this.#teardownSampled();
    this.synthVoice?.dispose(); this.synthVoice = null;
    this.nodes = null; this.ctx = null;
    if (!n) return;
    for (const s of n.sources) { try { s.stop(); } catch { /* already stopped */ } try { s.disconnect(); } catch { /* ignore */ } }
    try { n.master.disconnect(); } catch { /* ignore */ }
    ctx.close?.()?.catch?.(() => {});
  }
}
