import assert from "node:assert/strict";
import test from "node:test";
import { createFrameLoop } from "../../web/frame-loop.js";

function makeQueue() {
  const callbacks = [];
  return { callbacks, requestFrame: (callback) => callbacks.push(callback) };
}

test("a fixed-step exception releases input, reports recovery, and stops scheduling", () => {
  const queue = makeQueue();
  const heldInputs = new Set(["KeyW", "KeyB"]);
  const reports = [];
  let steps = 0;
  let renders = 0;
  const loop = createFrameLoop({
    stepSeconds: 1 / 60,
    requestFrame: queue.requestFrame,
    fixedStep() { steps++; throw new Error("synthetic step failure"); },
    render() { renders++; },
    onFault(error) {
      heldInputs.clear();
      reports.push({ message: error.message, recoveryVisible: true });
    },
  });

  loop.start();
  queue.callbacks.shift()(0);
  queue.callbacks.shift()(20);
  loop.start();

  assert.equal(steps, 1);
  assert.equal(renders, 1);
  assert.deepEqual([...heldInputs], []);
  assert.deepEqual(reports, [{ message: "synthetic step failure", recoveryVisible: true }]);
  assert.equal(loop.faulted, true);
  assert.equal(queue.callbacks.length, 0);
});

test("a render exception faults once and does not keep retrying the broken renderer", () => {
  const queue = makeQueue();
  const failures = [];
  let renders = 0;
  const loop = createFrameLoop({
    stepSeconds: 1 / 60,
    requestFrame: queue.requestFrame,
    fixedStep() {},
    render() { renders++; throw new Error("synthetic render failure"); },
    onFault(error) { failures.push(error.message); },
  });

  loop.start();
  queue.callbacks.shift()(0);
  assert.equal(renders, 1);
  assert.deepEqual(failures, ["synthetic render failure"]);
  assert.equal(loop.faulted, true);
  assert.equal(queue.callbacks.length, 0);
});
