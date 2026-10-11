export function createFrameLoop({ fixedStep, render, stepSeconds, onFault, requestFrame = (callback) => requestAnimationFrame(callback) }) {
  if (typeof fixedStep !== "function" || typeof render !== "function"
      || typeof onFault !== "function" || typeof requestFrame !== "function"
      || !Number.isFinite(stepSeconds) || stepSeconds <= 0) {
    throw new TypeError("frame loop requires valid callbacks and a positive step interval");
  }

  let previous = null;
  let accumulator = 0;
  let running = false;
  let faulted = false;

  function fail(error) {
    if (faulted) return;
    faulted = true;
    running = false;
    try { onFault(error); } catch { /* Keep the failed loop stopped even if reporting fails. */ }
  }

  function schedule() {
    try { requestFrame(frame); }
    catch (error) { fail(error); }
  }

  function frame(now) {
    if (!running || faulted) return;
    try {
      if (!Number.isFinite(now)) throw new Error("browser supplied an invalid frame timestamp");
      if (previous === null) previous = now;
      accumulator += Math.min((now - previous) / 1000, 0.25);
      previous = now;
      while (accumulator >= stepSeconds) {
        fixedStep();
        accumulator -= stepSeconds;
      }
      render();
    } catch (error) {
      fail(error);
      return;
    }
    schedule();
  }

  return {
    start() {
      if (running || faulted) return;
      running = true;
      schedule();
    },
    stop() { running = false; },
    get faulted() { return faulted; },
  };
}

export function createFrameFaultHandler({ releaseInputs, releaseNetwork, report }) {
  if (typeof releaseInputs !== "function" || typeof releaseNetwork !== "function" || typeof report !== "function") {
    throw new TypeError("frame fault handler requires input, network, and reporting callbacks");
  }
  let handled = false;
  return (error) => {
    if (handled) return false;
    handled = true;
    for (const action of [releaseInputs, releaseNetwork, report]) {
      try { action(error); } catch { /* Fault recovery must continue through every cleanup step. */ }
    }
    return true;
  };
}

// --- Chase-camera yaw smoothing (tests/feel) ---
// Lives in this module because the pinned static-route sets of the packaging
// verifiers do not allow new web files. The observed camera follows the body
// yaw with cam += 0.365 * (body - cam) per 1/32 s tick (decay 0.635 per tick).

export const CHASE_CAMERA_TICK_SECONDS = 1 / 32;
export const CHASE_CAMERA_DECAY_PER_TICK = 0.635;

/** Wrap an angle difference into (-PI, PI]. */
export function shortestAngle(delta) {
  const turn = 2 * Math.PI;
  let wrapped = delta % turn;
  if (wrapped > Math.PI) wrapped -= turn;
  else if (wrapped <= -Math.PI) wrapped += turn;
  return wrapped;
}

/** Frame-rate independent gain for a frame of `dt` seconds: 1 - 0.635^(dt * 32). */
export function chaseCameraAlpha(dt) {
  if (!Number.isFinite(dt) || dt <= 0) return 0;
  return 1 - Math.pow(CHASE_CAMERA_DECAY_PER_TICK, dt / CHASE_CAMERA_TICK_SECONDS);
}

/**
 * Stateful smoother. `update(bodyHeading, dt, key, position)` returns the camera
 * yaw. The camera snaps to the body (no smoothing) on the first call, when
 * `key` changes (respawn, track change, new race), after a long stall, or when
 * the body yaw or position jumps further than any driving could produce.
 */
export function createChaseCameraSmoother({ maxYawRate = 8, maxSpeed = 200, stallSeconds = 0.5 } = {}) {
  let camera = null;
  let lastBody = null;
  let lastPosition = null;
  let lastKey;
  return {
    update(bodyHeading, dt, key = null, position = null) {
      if (!Number.isFinite(bodyHeading)) return camera ?? 0;
      let reset = camera === null || key !== lastKey || !Number.isFinite(dt) || dt <= 0 || dt > stallSeconds;
      if (!reset && lastBody !== null) {
        reset = Math.abs(shortestAngle(bodyHeading - lastBody)) > Math.max(1.0, maxYawRate * dt);
      }
      if (!reset && position && lastPosition) {
        const jump = Math.hypot(position[0] - lastPosition[0], position[1] - lastPosition[1]);
        reset = jump > 20 + maxSpeed * dt;
      }
      if (reset) {
        camera = bodyHeading;
      } else {
        camera += chaseCameraAlpha(dt) * shortestAngle(bodyHeading - camera);
      }
      lastBody = bodyHeading;
      lastPosition = position ? [position[0], position[1]] : null;
      lastKey = key;
      return camera;
    },
    reset() { camera = null; lastBody = null; lastPosition = null; },
    get heading() { return camera; },
  };
}
