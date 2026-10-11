// Bounded client-intent history for server-acknowledged reconciliation.
// This stores inputs only; restoring simulation state is owned by the caller.
export class InputHistory {
  constructor({ firstSequence = 0, capacity = 128 } = {}) {
    if (!Number.isInteger(firstSequence) || firstSequence < 0 || firstSequence > 0xffffffff) {
      throw new RangeError("firstSequence must be an unsigned 32-bit integer");
    }
    if (!Number.isInteger(capacity) || capacity < 1 || capacity > 4096) {
      throw new RangeError("capacity must be between 1 and 4096");
    }
    this.nextSequence = firstSequence >>> 0;
    this.capacity = capacity;
    this.pending = [];
    this.lastAck = null;
    this.lastAckServerTick = null;
  }

  record(input) {
    const item = Object.freeze({ ...input, sequence: this.nextSequence });
    this.nextSequence = (this.nextSequence + 1) >>> 0;
    this.pending.push(item);
    if (this.pending.length > this.capacity) this.pending.shift();
    return item;
  }

  acknowledgeApplied(sequence, serverTick) {
    if (!Number.isInteger(sequence) || sequence < 0 || sequence > 0xffffffff
        || !Number.isSafeInteger(serverTick) || serverTick < 0) return false;
    sequence >>>= 0;
    if (this.lastAckServerTick !== null && serverTick < this.lastAckServerTick) return false;
    if (this.lastAck === sequence) {
      this.lastAckServerTick = serverTick;
      return true;
    }
    if (this.lastAck !== null && !isNewerSequence(sequence, this.lastAck)) return false;
    const index = this.pending.findIndex((item) => item.sequence === sequence);
    if (index < 0) return false;
    this.pending.splice(0, index + 1);
    this.lastAck = sequence;
    this.lastAckServerTick = serverTick;
    return true;
  }

  acknowledgeSnapshot(snapshot) {
    if (!snapshot) return false;
    return this.acknowledgeApplied(
      snapshot.ack_applied_input_sequence,
      snapshot.ack_applied_server_tick,
    );
  }

  replayInputs() {
    return this.pending.slice();
  }

  reset(firstSequence = 0) {
    this.nextSequence = firstSequence >>> 0;
    this.pending = [];
    this.lastAck = null;
    this.lastAckServerTick = null;
  }
}

function isNewerSequence(candidate, previous) {
  const delta = (candidate - previous) >>> 0;
  return delta !== 0 && delta < 0x80000000;
}

/**
 * Gear-change requests are edge events, but the simulation and the network
 * sample key levels (60 Hz frames, one network message per few frames, 32 Hz
 * server steps). A press shorter than the sampling interval would vanish, so
 * every rising edge is kept as a pending request until the caller reports it
 * delivered with `consume()`. Requests are per direction (`up`, `down`) so a
 * future manual gearbox can ask for several shifts without changing the wire.
 */
export function createShiftLatch() {
  const pending = { up: false, down: false };
  const previous = { up: false, down: false };
  return {
    /** Record a discrete press (keydown, gamepad edge) of "up" or "down". */
    press(direction) {
      if (direction === "up" || direction === "down") pending[direction] = true;
    },
    /** Feed the current key levels once per frame; rising edges become pending requests. */
    observe(levels) {
      for (const direction of ["up", "down"]) {
        const level = !!levels?.[direction];
        if (level && !previous[direction]) pending[direction] = true;
        previous[direction] = level;
      }
    },
    /** Levels merged with pending requests, as `{ shift_up, shift_down }`. */
    merged(levels) {
      return { shift_up: !!levels?.up || pending.up, shift_down: !!levels?.down || pending.down };
    },
    /** The merged requests were sent or applied: drop them. */
    consume() { pending.up = false; pending.down = false; },
    clear() { this.consume(); previous.up = false; previous.down = false; },
    get pending() { return { ...pending }; },
  };
}
