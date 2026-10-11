import assert from "node:assert/strict";
import test from "node:test";
import { InputHistory } from "../../web/input-history.js";

test("received sequence does not prune history; applied sequence and tick do", () => {
  const history = new InputHistory({ capacity: 4 });
  const first = history.record({ throttle: 1, client_tick: 10 });
  const second = history.record({ throttle: 0, client_tick: 11 });
  const third = history.record({ steer: 1, client_tick: 12 });
  assert.deepEqual([first.sequence, second.sequence, third.sequence], [0, 1, 2]);
  assert.equal(history.acknowledgeSnapshot({
    ack_input_sequence: 2,
    ack_applied_input_sequence: null,
    ack_applied_server_tick: null,
  }), false);
  assert.deepEqual(history.replayInputs(), [first, second, third]);
  assert.equal(history.lastAck, null);
  assert.equal(history.lastAckServerTick, null);

  assert.equal(history.acknowledgeSnapshot({
    ack_input_sequence: 2,
    ack_applied_input_sequence: 1,
    ack_applied_server_tick: 41,
  }), true);
  assert.deepEqual(history.replayInputs(), [third]);
  assert.equal(history.lastAck, 1);
  assert.equal(history.lastAckServerTick, 41);
  assert.equal(history.acknowledgeApplied(1, 42), true);
  assert.equal(history.lastAckServerTick, 42);
  assert.equal(history.acknowledgeApplied(1, 40), false);
  assert.equal(history.acknowledgeApplied(9, 43), false);
  assert.deepEqual(history.replayInputs(), [third]);
  assert.equal(history.acknowledgeApplied(2, 43), true);
  assert.deepEqual(history.replayInputs(), []);

  history.reset();
  assert.equal(history.lastAck, null);
  assert.equal(history.lastAckServerTick, null);
  assert.deepEqual(history.replayInputs(), []);
});

test("sequence generation wraps and bounded history fails closed for evicted acknowledgements", () => {
  const history = new InputHistory({ firstSequence: 0xfffffffe, capacity: 2 });
  assert.equal(history.record({}).sequence, 0xfffffffe);
  assert.equal(history.record({}).sequence, 0xffffffff);
  const wrapped = history.record({});
  assert.equal(wrapped.sequence, 0);
  assert.equal(history.acknowledgeApplied(0xfffffffe, 4), false);
  assert.equal(history.acknowledgeApplied(0, 5), true);
  assert.equal(history.lastAckServerTick, 5);
  assert.deepEqual(history.replayInputs(), []);
});
