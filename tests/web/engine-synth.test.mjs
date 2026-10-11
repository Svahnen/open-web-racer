import test from "node:test";
import assert from "node:assert/strict";
import { EngineSynthVoice, MAX_HARMONICS, engineSynthParams, parseEngineSound } from "../../web/engine-synth.js";
import { GameAudio } from "../../web/audio.js";
import { parseCatalog } from "../../web/project-cars.js";

const RAW = Object.freeze({
  idle_rpm: 900, redline_rpm: 7200, cylinders: 6, harmonics: [1, 0.5, 0.25, 0.1],
  filter_hz_idle: 500, filter_hz_redline: 3400, noise_gain: 0.08, gain_idle: 0.3, gain_redline: 0.6,
});

function param() {
  return { value: 0, calls: [], setTargetAtTime(v, t) { this.calls.push([v, t]); this.value = v; } };
}
function node(extra = {}) {
  return { connect() {}, disconnect() {}, start() { this.started = true; }, stop() { this.stopped = true; }, gain: param(), frequency: param(), detune: param(), Q: param(), ...extra };
}
function contextClass(log) {
  return class FakeAudioContext {
    constructor() { this.state = "running"; this.sampleRate = 8000; this.currentTime = 0; this.destination = node(); log.contexts.push(this); }
    resume() { return Promise.resolve(); }
    close() { this.state = "closed"; return Promise.resolve(); }
    createGain() { const n = node(); log.gains.push(n); return n; }
    createOscillator() { const n = node(); log.oscs.push(n); return n; }
    createBiquadFilter() { const n = node(); log.filters.push(n); return n; }
    createBufferSource() { const n = node(); log.sources.push(n); return n; }
    createBuffer(_c, length) { const d = new Float32Array(length); return { getChannelData: () => d }; }
  };
}

test("parseEngineSound accepts a valid block and rejects malformed ones", () => {
  const s = parseEngineSound(RAW);
  assert.equal(s.cylinders, 6);
  assert.deepEqual(s.harmonics, [1, 0.5, 0.25, 0.1]);
  assert.equal(parseEngineSound(null), null);
  assert.equal(parseEngineSound({ ...RAW, redline_rpm: 800 }), null);
  assert.equal(parseEngineSound({ ...RAW, cylinders: 0 }), null);
  assert.equal(parseEngineSound({ ...RAW, cylinders: 2.5 }), null);
  assert.equal(parseEngineSound({ ...RAW, harmonics: [] }), null);
  assert.equal(parseEngineSound({ ...RAW, harmonics: new Array(MAX_HARMONICS + 1).fill(0.1) }), null);
  assert.equal(parseEngineSound({ ...RAW, noise_gain: NaN }), null);
  assert.deepEqual(parseEngineSound({ ...RAW, harmonics: [3, -1] }).harmonics, [1, 0]);
});

test("pitch follows engine speed and cylinder count", () => {
  const s = parseEngineSound(RAW);
  const idle = engineSynthParams(s, { rpm01: 0, throttle: 0 });
  const top = engineSynthParams(s, { rpm01: 1, throttle: 1 });
  // 6 cylinders fire three times per revolution: 900 rpm is 45 Hz.
  assert.ok(Math.abs(idle.fundamentalHz - 45) < 1e-9);
  assert.ok(Math.abs(top.fundamentalHz - 360) < 1e-9);
  assert.equal(idle.rpm, 900);
  assert.equal(top.rpm, 7200);
  assert.ok(top.filterHz > idle.filterHz);
  assert.ok(top.harmonicGains[0] > idle.harmonicGains[0]);
  const four = parseEngineSound({ ...RAW, cylinders: 4 });
  assert.ok(Math.abs(engineSynthParams(four, { rpm01: 1 }).fundamentalHz - 240) < 1e-9);
});

test("throttle opens the filter and raises the level; bad input is tolerated", () => {
  const s = parseEngineSound(RAW);
  const coast = engineSynthParams(s, { rpm01: 0.5, throttle: 0 });
  const load = engineSynthParams(s, { rpm01: 0.5, throttle: 1 });
  const sum = (a) => a.reduce((x, y) => x + y, 0);
  assert.ok(load.filterHz > coast.filterHz);
  assert.ok(sum(load.harmonicGains) > sum(coast.harmonicGains));
  assert.ok(load.noiseGain > coast.noiseGain);
  const bad = engineSynthParams(s, { rpm01: NaN, throttle: NaN });
  assert.equal(bad.rpm, 900);
  assert.ok(bad.harmonicGains.every(Number.isFinite));
  assert.equal(engineSynthParams(s, { rpm01: 9 }).rpm, 7200);
});

test("harmonics above the usable band are dropped and the mix is normalised", () => {
  const s = parseEngineSound({ ...RAW, harmonics: [1, 1, 1, 1, 1, 1, 1, 1], cylinders: 16, idle_rpm: 900, redline_rpm: 9000 });
  const p = engineSynthParams(s, { rpm01: 1, throttle: 1 }, 8000);
  assert.ok(p.harmonicGains.some((g) => g === 0));
  assert.ok(p.harmonicGains[0] > 0);
  const level = 0.6;
  assert.ok(Math.abs(p.harmonicGains.reduce((a, b) => a + b, 0) - level) < 1e-9);
});

test("the voice drives its oscillators and disposes every source", () => {
  const log = { contexts: [], gains: [], oscs: [], filters: [], sources: [] };
  const ctx = new (contextClass(log))();
  const voice = new EngineSynthVoice({ ctx, destination: node(), noiseBuffer: {}, sound: parseEngineSound(RAW) });
  assert.equal(log.oscs.length, 4);
  const p = voice.update({ rpm01: 0.5, throttle: 1 });
  assert.ok(Math.abs(log.oscs[1].frequency.value - 2 * p.fundamentalHz) < 1e-9);
  assert.ok(log.oscs.every((o) => o.started));
  voice.dispose();
  assert.ok(log.oscs.every((o) => o.stopped));
  assert.ok(log.sources.every((s) => s.stopped));
});

test("GameAudio uses the project engine voice, falls back, and yields to owner sounds", () => {
  const log = { contexts: [], gains: [], oscs: [], filters: [], sources: [] };
  const audio = new GameAudio({ AudioContextClass: contextClass(log) });
  audio.setEngineSound(RAW); // before unlock: kept until the graph exists
  audio.unlock();
  assert.ok(audio.synthVoice);
  const classicOscillators = 2;
  assert.equal(log.oscs.length, classicOscillators + 4);
  audio.setEngineState({ rpm01: 0.8, throttle: 1, speed: 20 }, 1 / 60);
  assert.ok(log.oscs[classicOscillators].frequency.value > 100);
  assert.equal(audio.nodes.toneGain.gain.value, 0);
  audio.setEngineSound({ nonsense: true });
  assert.equal(audio.synthVoice, null);
  audio.setEngineState({ rpm01: 0.8, throttle: 1 }, 1 / 60);
  assert.ok(audio.nodes.toneGain.gain.value > 0);
  audio.setEngineSound(RAW);
  assert.ok(audio.synthVoice);
  audio.dispose();
  assert.equal(audio.synthVoice, null);
});

test("the project car catalogue carries the engine voice through", () => {
  const catalog = parseCatalog({ cars: [
    { id: "a-car", name: "A", paints: [{ id: "red", body: "#ff0000" }], engine_sound: RAW },
    { id: "b-car", name: "B", paints: [{ id: "red", body: "#ff0000" }], engine_sound: null },
    { id: "c-car", name: "C", paints: [{ id: "red", body: "#ff0000" }] },
  ] });
  assert.deepEqual(catalog.cars[0].engineSound, RAW);
  assert.equal(catalog.cars[1].engineSound, null);
  assert.equal(catalog.cars[2].engineSound, null);
});
