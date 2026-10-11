import assert from "node:assert/strict";
import test from "node:test";
import { NO_CAR, packPicks, parseCatalog, rosterPair, selectionIndices, wireFields, wireIndices } from "../../web/project-cars.js";

const catalog = parseCatalog({
  cars: [
    { id: "alpha-one", name: "Alpha One", paints: [{ id: "signal-red", body: "#c82622" }, { id: "plum", body: "#783078" }] },
    { id: "beta-two", name: "Beta Two", paints: [{ id: "plum", body: "#783078" }, { id: "graphite", body: "#303030" }, { id: "sand", body: "#c0b090" }] },
  ],
});

test("a selection maps to catalogue indices, unknown paints to the first paint", () => {
  assert.deepEqual(selectionIndices(catalog, { car: "beta-two", paint: "graphite" }), [1, 1]);
  assert.deepEqual(selectionIndices(catalog, { car: "beta-two", paint: "missing" }), [1, 0]);
  assert.equal(selectionIndices(catalog, { car: "missing", paint: "plum" }), null);
  assert.equal(selectionIndices(catalog, null), null);
});

test("wire choices are validated against the catalogue", () => {
  assert.deepEqual(wireIndices(catalog, "beta-two", 2), [1, 2]);
  assert.equal(wireIndices(catalog, "beta-two", 3), null);
  assert.equal(wireIndices(catalog, "beta-two", -1), null);
  assert.equal(wireIndices(catalog, "beta-two", 1.5), null);
  assert.equal(wireIndices(catalog, "", 0), null);
  assert.equal(wireIndices(catalog, "gamma", 0), null);
  assert.equal(wireIndices(catalog, undefined, undefined), null);
});

test("rooms are told the picked model id and paint index", () => {
  assert.deepEqual(wireFields(catalog, { car: "beta-two", paint: "sand" }), { car_model: "beta-two", car_variant: 2 });
  assert.deepEqual(wireFields(null, { car: "beta-two", paint: "sand" }), { car_model: "", car_variant: 0 });
  assert.deepEqual(wireFields(catalog, { car: "nope", paint: "sand" }), { car_model: "", car_variant: 0 });
});

test("picks are packed as byte pairs with a marker for vehicles without a car", () => {
  const bytes = packPicks([[1, 2], null, [0, 1]]);
  assert.deepEqual([...bytes], [1, 2, NO_CAR, NO_CAR, 0, 1]);
  assert.equal(packPicks([]).length, 0);
  const roster = Uint8Array.from([0, 3, 1, 4, 2, 5]);
  assert.deepEqual(rosterPair(roster, 0), [0, 3]);
  assert.deepEqual(rosterPair(roster, 2), [2, 5]);
  assert.equal(rosterPair(roster, 3), null);
  assert.equal(rosterPair(null, 0), null);
});
