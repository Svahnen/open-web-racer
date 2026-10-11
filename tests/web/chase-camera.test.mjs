import assert from "node:assert/strict";
import test from "node:test";
import { chaseCameraAlpha, createChaseCameraSmoother, shortestAngle } from "../../web/frame-loop.js";

const deg = (rad) => rad * 180 / Math.PI;

// Body turns at `rate` rad/s for `hold` s then stops; returns the peak
// body-minus-camera offset (deg) and the time after release when it is below 1 deg.
function turnAndRelease(frameHz, rate, hold) {
  const smoother = createChaseCameraSmoother();
  const dt = 1 / frameHz;
  let body = 0;
  smoother.update(body, dt, "k");
  let peak = 0;
  let settled = null;
  for (let i = 1; i <= Math.round((hold + 1) * frameHz); i++) {
    const t = i * dt;
    if (t <= hold) body += rate * dt;
    const camera = smoother.update(body, dt, "k");
    const offset = deg(body - camera);
    peak = Math.max(peak, offset);
    if (t > hold + 1e-9 && settled === null && offset < 1) settled = t - hold;
  }
  return { peak, settled };
}

test("alpha matches 1 - 0.635^(dt*32) and composes across frame sizes", () => {
  assert.ok(Math.abs(chaseCameraAlpha(1 / 32) - 0.365) < 1e-12);
  assert.equal(chaseCameraAlpha(0), 0);
  assert.equal(chaseCameraAlpha(Number.NaN), 0);
  const two = 1 - (1 - chaseCameraAlpha(1 / 64)) ** 2;
  assert.ok(Math.abs(two - chaseCameraAlpha(1 / 32)) < 1e-12);
});

test("peak lag and settle time match the observed 1.73 rad/s turn", () => {
  const at32 = turnAndRelease(32, 1.73, 0.6);
  assert.ok(at32.peak >= 5.3 && at32.peak <= 5.9, `32 Hz peak ${at32.peak}`);
  assert.ok(at32.settled <= 0.16, `32 Hz settle ${at32.settled}`);
  // The decay is frame-rate independent; the steady lag grows slightly with
  // frame rate (continuous-time limit 2.2 ticks versus 1.74 ticks at one
  // frame per tick), so higher rates are checked against a wider band.
  for (const hz of [60, 144]) {
    const run = turnAndRelease(hz, 1.73, 0.6);
    assert.ok(run.peak >= 5.3 && run.peak <= 6.9, `${hz} Hz peak ${run.peak}`);
    assert.ok(run.settled <= 0.2, `${hz} Hz settle ${run.settled}`);
  }
  const slow = turnAndRelease(32, 1.0, 0.6);
  assert.ok(slow.peak >= 3.0 && slow.peak <= 3.8, `1 rad/s peak ${slow.peak}`);
});

test("camera follows the shortest way across the +-PI seam", () => {
  const smoother = createChaseCameraSmoother();
  smoother.update(Math.PI - 0.05, 1 / 60, "k");
  const next = smoother.update(-Math.PI + 0.05, 1 / 60, "k");
  // Body moved +0.1 rad through the seam; the camera must move forward, not by ~2 PI.
  assert.ok(Math.abs(shortestAngle(next - (Math.PI - 0.05))) < 0.1);
  assert.ok(Math.abs(shortestAngle(-Math.PI + 0.05 - next)) < 0.1);
  assert.ok(Math.abs(shortestAngle(3 * Math.PI) - Math.PI) < 1e-9);
});

test("respawn, key change, stalls and teleports snap the camera to the body", () => {
  const smoother = createChaseCameraSmoother();
  smoother.update(0, 1 / 60, "a", [0, 0]);
  smoother.update(0.05, 1 / 60, "a", [0.5, 0]);
  assert.notEqual(smoother.heading, 0.05);
  assert.equal(smoother.update(2.5, 1 / 60, "b", [0, 0]), 2.5); // key change
  smoother.update(2.5, 1 / 60, "b", [0, 0]);
  assert.equal(smoother.update(-1, 1 / 60, "b", [0, 0]), -1); // yaw jump (respawn)
  assert.equal(smoother.update(-1, 1 / 60, "b", [500, 0]), -1); // position jump
  assert.equal(smoother.update(0.3, 2, "b", [500, 0]), 0.3); // long stall
  assert.equal(smoother.update(Number.NaN, 1 / 60, "b"), 0.3); // bad input ignored
});
