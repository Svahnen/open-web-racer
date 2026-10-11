import assert from "node:assert/strict";
import test from "node:test";
import {
  CONTROLS_HINT,
  CONTROLS_HINT_MANUAL,
  controlsHintFor,
  GEAR_HINT_NEUTRAL_THROTTLE,
  GEAR_HINT_REVERSE_FROM_FORWARD,
  GEAR_HINT_REVERSE_FROM_NEUTRAL,
  createGearHintTracker,
  gearKind,
} from "../../web/hud-model.js";
import { createShiftLatch } from "../../web/input-history.js";

const DT = 1 / 60;
const N = 251;
const R = 250;

function run(tracker, seconds, state) {
  let last = { hint: null, autoShiftDown: false };
  const downs = [];
  for (let i = 0; i < Math.round(seconds / DT); i += 1) {
    last = tracker.update({ dt: DT, live: true, throttle: 0, brake: 0, speed: 0, ...state });
    if (last.autoShiftDown) downs.push(i);
  }
  return { last, downs };
}

test("controls hint names the shift keys and the reverse procedure", () => {
  assert.match(CONTROLS_HINT, /Z\/A/);
  assert.match(CONTROLS_HINT, /Z Z at standstill: reverse/);
});

test("holding accelerate in neutral for over a second shows the 1st-gear hint", () => {
  const tracker = createGearHintTracker();
  assert.equal(run(tracker, 0.9, { gear: N, throttle: 1 }).last.hint, null);
  assert.equal(run(tracker, 0.3, { gear: N, throttle: 1 }).last.hint, GEAR_HINT_NEUTRAL_THROTTLE);
  // Releasing the pedal, or selecting a gear, clears it.
  assert.equal(run(tracker, 0.1, { gear: N, throttle: 0 }).last.hint, null);
  run(tracker, 1.2, { gear: N, throttle: 1 });
  assert.equal(run(tracker, DT, { gear: 1, throttle: 1 }).last.hint, null);
});

test("no hint in reverse, in forward gears under throttle, when not live, or without a gearbox", () => {
  const tracker = createGearHintTracker();
  assert.equal(run(tracker, 3, { gear: R, throttle: 1 }).last.hint, null);
  assert.equal(run(tracker, 3, { gear: 2, throttle: 1, speed: 10 }).last.hint, null);
  assert.equal(run(tracker, 3, { gear: 0, throttle: 1 }).last.hint, null);
  const idle = createGearHintTracker();
  for (let i = 0; i < 200; i += 1) {
    assert.equal(idle.update({ dt: DT, live: false, gear: N, throttle: 1, brake: 0, speed: 0 }).hint, null);
  }
});

test("brake held at standstill explains how to reverse; moving or throttle does not", () => {
  const fwd = createGearHintTracker();
  assert.equal(run(fwd, 1.2, { gear: 1, brake: 1 }).last.hint, null);
  assert.equal(run(fwd, 0.6, { gear: 1, brake: 1 }).last.hint, GEAR_HINT_REVERSE_FROM_FORWARD);
  const neutral = createGearHintTracker();
  assert.equal(run(neutral, 1.8, { gear: N, brake: 1 }).last.hint, GEAR_HINT_REVERSE_FROM_NEUTRAL);
  const moving = createGearHintTracker();
  assert.equal(run(moving, 3, { gear: 1, brake: 1, speed: 8 }).last.hint, null);
  const both = createGearHintTracker();
  assert.equal(run(both, 3, { gear: 1, brake: 1, throttle: 1 }).last.hint, null);
});

test("auto reverse leaves reversing to the simulation and shows no hint", () => {
  const on = createGearHintTracker();
  const result = run(on, 3, { gear: 1, brake: 1, autoReverse: true });
  assert.equal(result.downs.length, 0);
  assert.equal(result.last.hint, null);
  assert.equal(run(on, 3, { gear: N, throttle: 1, autoReverse: true }).last.hint, null);
  // Without the option the hint still teaches the shift.
  const off = createGearHintTracker();
  assert.equal(run(off, 3, { gear: 1, brake: 1 }).last.hint, GEAR_HINT_REVERSE_FROM_FORWARD);
});

test("auto reverse controls hint describes brake-to-reverse for both gearboxes", () => {
  assert.match(controlsHintFor("automatic", true), /reverse/);
  assert.doesNotMatch(controlsHintFor("automatic", true), /Z Z/);
  assert.match(controlsHintFor("manual", true), /Manual gearbox/);
  assert.equal(controlsHintFor("manual", false), CONTROLS_HINT_MANUAL);
});

test("gear kind distinguishes R, N and forward gears", () => {
  assert.equal(gearKind(R), "reverse");
  assert.equal(gearKind(N), "neutral");
  assert.equal(gearKind(3), "forward");
  assert.equal(gearKind(0), "none");
  assert.equal(gearKind(undefined), "none");
});

test("a shift press shorter than the sampling interval still reaches the next send", () => {
  const latch = createShiftLatch();
  // Key down and up between two network sends: levels never observed high.
  latch.press("down");
  const first = latch.merged({ up: false, down: false });
  assert.deepEqual(first, { shift_up: false, shift_down: true });
  latch.consume();
  assert.deepEqual(latch.merged({ up: false, down: false }), { shift_up: false, shift_down: false });
});

test("a held key is one pending request, not one per frame", () => {
  const latch = createShiftLatch();
  latch.observe({ up: false, down: true });
  assert.equal(latch.pending.down, true);
  latch.consume();
  for (let i = 0; i < 10; i += 1) {
    latch.observe({ up: false, down: true });
    assert.equal(latch.pending.down, false);
    assert.equal(latch.merged({ up: false, down: true }).shift_down, true);
  }
  latch.observe({ up: false, down: false });
  latch.observe({ up: false, down: true });
  assert.equal(latch.pending.down, true);
});

test("pending requests survive frames without a send and clear on release of focus", () => {
  const latch = createShiftLatch();
  latch.press("up");
  for (let i = 0; i < 5; i += 1) latch.observe({ up: false, down: false });
  assert.equal(latch.merged({}).shift_up, true);
  latch.clear();
  assert.equal(latch.merged({}).shift_up, false);
  latch.press("sideways");
  assert.deepEqual(latch.pending, { up: false, down: false });
});
