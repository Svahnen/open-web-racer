import test from "node:test";
import assert from "node:assert/strict";
import { readSpeedUnit, speedUnitPreferenceLocation } from "../../web/display-preferences.js";

test("speed unit query preference accepts only kmh or mph and defaults to kmh", () => {
  assert.equal(readSpeedUnit("?speedUnit=mph"), "mph");
  assert.equal(readSpeedUnit("?speedUnit=raw"), "raw");
  assert.equal(readSpeedUnit("?speedUnit=kmh"), "kmh");
  assert.equal(readSpeedUnit("?speedUnit=knots"), "kmh");
  assert.equal(readSpeedUnit(""), "kmh");
});

test("speed unit preference updates its query while preserving other URL state", () => {
  assert.equal(
    speedUnitPreferenceLocation("https://example.test/?practice=local&camera=wide#race", "mph"),
    "/?practice=local&camera=wide&speedUnit=mph#race",
  );
});
