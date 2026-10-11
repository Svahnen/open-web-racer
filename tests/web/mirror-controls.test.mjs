import test from "node:test";
import assert from "node:assert/strict";
import { mirrorVisibilityAfterKeydown } from "../../web/mirror-controls.js";

function keyEvent({ code = "F7", repeat = false, ctrlKey = false, metaKey = false, altKey = false } = {}) {
  return { code, repeat, ctrlKey, metaKey, altKey };
}

test("F7 toggles the project mirror once per key press", () => {
  let visible = true;
  visible = mirrorVisibilityAfterKeydown(visible, keyEvent(), false);
  assert.equal(visible, false);
  assert.equal(mirrorVisibilityAfterKeydown(visible, keyEvent({ repeat: true }), false), null);
  visible = mirrorVisibilityAfterKeydown(visible, keyEvent(), false);
  assert.equal(visible, true);
});

test("mirror toggle ignores editable targets, modifiers, and other keys", () => {
  assert.equal(mirrorVisibilityAfterKeydown(true, keyEvent(), true), null);
  assert.equal(mirrorVisibilityAfterKeydown(true, keyEvent({ ctrlKey: true }), false), null);
  assert.equal(mirrorVisibilityAfterKeydown(true, keyEvent({ metaKey: true }), false), null);
  assert.equal(mirrorVisibilityAfterKeydown(true, keyEvent({ altKey: true }), false), null);
  assert.equal(mirrorVisibilityAfterKeydown(true, keyEvent({ code: "KeyF" }), false), null);
});
