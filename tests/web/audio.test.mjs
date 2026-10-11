import test from "node:test";
import assert from "node:assert/strict";
import { GameAudio, engineParams, lateralSpeed, skidLevel } from "../../web/audio.js";
import { clampVolume, readSoundPreference, soundPreferenceLocation } from "../../web/display-preferences.js";

function param() {
  return { value: 0, calls: [], setTargetAtTime(v) { this.calls.push(v); this.value = v; }, setValueAtTime() {}, exponentialRampToValueAtTime() {} };
}
function node(extra = {}) {
  return { connect() {}, disconnect() {}, start() { this.started = true; }, stop() { this.stopped = true; }, gain: param(), frequency: param(), detune: param(), Q: param(), ...extra };
}
function makeContextClass(log) {
  return class FakeAudioContext {
    constructor() { this.state = "suspended"; this.sampleRate = 8000; this.currentTime = 0; this.destination = node(); log.contexts.push(this); }
    resume() { this.state = "running"; return Promise.resolve(); }
    close() { this.state = "closed"; return Promise.resolve(); }
    createGain() { const n = node(); log.nodes.push(n); return n; }
    createOscillator() { const n = node(); log.nodes.push(n); return n; }
    createBiquadFilter() { const n = node(); log.nodes.push(n); return n; }
    createBufferSource() { const n = node(); log.nodes.push(n); return n; }
    createBuffer(_c, length) { const d = new Float32Array(length); return { getChannelData: () => d }; }
  };
}

test("engine pitch and gain rise with speed and throttle, rpm01 overrides speed", () => {
  const idle = engineParams({ speed: 0, throttle: 0 });
  const fast = engineParams({ speed: 60, throttle: 1 });
  assert.ok(fast.baseHz > idle.baseHz && fast.gain > idle.gain && fast.filterHz > idle.filterHz);
  assert.equal(engineParams({ rpm01: 0.5, speed: 0 }).rpm, 0.5);
  assert.equal(engineParams({ speed: 1e9 }).rpm, 1);
  assert.equal(engineParams({ speed: NaN, throttle: NaN }).rpm, 0);
});

test("skid level needs speed and lateral slip, and tolerates missing data", () => {
  assert.equal(skidLevel(2, 30), 0);
  assert.equal(skidLevel(20, 2), 0);
  assert.equal(skidLevel(NaN, 30), 0);
  assert.ok(skidLevel(9, 30) > 0 && skidLevel(100, 30) === 1);
  assert.ok(Math.abs(lateralSpeed(0, 10, 0) - 10) < 1e-9);
  assert.ok(Number.isNaN(lateralSpeed(undefined, 1, 0)));
});

test("audio stays inert until unlock, then builds, mutes, and disposes cleanly", () => {
  const log = { contexts: [], nodes: [] };
  const audio = new GameAudio({ AudioContextClass: makeContextClass(log), volume: 0.5 });
  audio.setEngineState({ speed: 10 }); // no-op before gesture
  assert.equal(log.contexts.length, 0);
  assert.equal(audio.unlock(), true);
  assert.equal(audio.unlock(), true);
  assert.equal(log.contexts.length, 1);
  assert.equal(log.contexts[0].state, "running");
  audio.setEngineState({ speed: 30, throttle: 1 });
  audio.setSkid(1);
  audio.playThump(1);
  audio.setMuted(true);
  assert.equal(audio.nodes.master.gain.value, 0);
  audio.setMuted(false);
  assert.equal(audio.nodes.master.gain.value, 0.5);
  const sources = audio.nodes.sources;
  audio.dispose();
  assert.ok(sources.every((s) => s.stopped));
  assert.equal(log.contexts[0].state, "closed");
  assert.equal(audio.nodes, null);
  assert.equal(audio.unlock(), true);
  assert.equal(log.contexts.length, 2);
});

test("audio is unsupported without an AudioContext", () => {
  const audio = new GameAudio({ AudioContextClass: undefined });
  assert.equal(audio.unlock(), false);
  audio.setEngineState({ speed: 1 });
  audio.dispose();
});

test("sound preference persists in the query and clamps bad input", () => {
  assert.deepEqual(readSoundPreference(""), { muted: false, volume: 0.7 });
  assert.deepEqual(readSoundPreference("?sound=off&volume=0.25"), { muted: true, volume: 0.25 });
  assert.equal(readSoundPreference("?volume=9").volume, 1);
  assert.equal(readSoundPreference("?volume=abc").volume, 0.7);
  assert.equal(clampVolume(null), 0.7);
  assert.equal(
    soundPreferenceLocation("https://example.test/?speedUnit=mph#race", { muted: true, volume: 0.333 }),
    "/?speedUnit=mph&sound=off&volume=0.33#race",
  );
  assert.equal(soundPreferenceLocation("https://example.test/?sound=off", { muted: false, volume: 1 }), "/?volume=1");
});
