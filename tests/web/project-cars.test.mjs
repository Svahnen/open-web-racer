import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import {
  MAX_FIELD,
  STORAGE_KEY,
  carOptions,
  defaultSelection,
  describeSelection,
  fieldOpponents,
  paintLabel,
  paintOptions,
  parseCatalog,
  readStored,
  resolveSelection,
  selectionFromSources,
  writeStored,
} from "../../web/project-cars.js";

const catalogJson = JSON.stringify({
  cars: [
    { id: "alpha-one", name: "Alpha One", paints: [{ id: "signal-red", body: "#c82622" }, { id: "plum", body: "#783078" }] },
    { id: "beta-two", name: "Beta Two", paints: [{ id: "plum", body: "#783078" }, { id: "graphite", body: "bad" }] },
  ],
});

test("catalogue parsing validates ids, names and colours", () => {
  const catalog = parseCatalog(catalogJson);
  assert.equal(catalog.cars.length, 2);
  assert.equal(catalog.cars[0].paints[0].name, "Signal Red");
  assert.equal(catalog.cars[1].paints[1].body, "#808080");
  assert.throws(() => parseCatalog("{\"cars\":[]}"), /empty/);
  assert.throws(() => parseCatalog({ cars: [{ id: "Bad Id", name: "x", paints: [{ id: "a" }] }] }), /id/);
  assert.throws(() => parseCatalog({ cars: [{ id: "a", name: " ", paints: [{ id: "a" }] }] }), /name/);
  assert.throws(() => parseCatalog({ cars: [{ id: "a", name: "A", paints: [] }] }), /paints/);
  assert.throws(() => parseCatalog({ cars: [{ id: "a", name: "A", paints: [{ id: "p" }] }, { id: "a", name: "B", paints: [{ id: "p" }] }] }), /id/);
});

test("the shipped generator output satisfies the catalogue contract", () => {
  const pack = JSON.parse(readFileSync(new URL("../../content/demo/cars/pack.json", import.meta.url), "utf8"));
  const cars = pack.documents.cars.map((path) => JSON.parse(readFileSync(new URL(`../../content/demo/cars/${path}`, import.meta.url), "utf8")));
  const catalog = parseCatalog({
    cars: cars.map((car) => ({
      id: car.id,
      name: car.name,
      paints: car.paint.options.map((option) => ({ id: option.name, body: "#000000" })),
    })),
  });
  assert.equal(catalog.cars.length, 4);
  assert.ok(catalog.cars.every((car) => car.paints.length >= 8));
});

test("selection falls back to existing cars and paints", () => {
  const catalog = parseCatalog(catalogJson);
  assert.deepEqual(defaultSelection(catalog), { car: "alpha-one", paint: "signal-red" });
  assert.deepEqual(resolveSelection(catalog, "beta-two", "plum"), { car: "beta-two", paint: "plum" });
  // A paint the model lacks falls back to its first paint; an unknown model to the default.
  assert.deepEqual(resolveSelection(catalog, "beta-two", "signal-red"), { car: "beta-two", paint: "plum" });
  assert.deepEqual(resolveSelection(catalog, "gone", "plum"), { car: "alpha-one", paint: "signal-red" });
});

test("query beats stored choice beats default", () => {
  const catalog = parseCatalog(catalogJson);
  const stored = { car: "beta-two", paint: "graphite" };
  assert.deepEqual(selectionFromSources(catalog, "?car=alpha-one&paint=plum", stored), { car: "alpha-one", paint: "plum" });
  assert.deepEqual(selectionFromSources(catalog, "?practice=local", stored), stored);
  assert.deepEqual(selectionFromSources(catalog, "", null), { car: "alpha-one", paint: "signal-red" });
  assert.deepEqual(selectionFromSources(catalog, "", { car: "unknown" }), { car: "alpha-one", paint: "signal-red" });
});

test("stored selection survives a round trip and tolerates a broken store", () => {
  const data = new Map();
  const storage = { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, v) };
  assert.equal(readStored(storage), null);
  assert.equal(writeStored(storage, { car: "alpha-one", paint: "plum" }), true);
  assert.ok(data.has(STORAGE_KEY));
  assert.deepEqual(readStored(storage), { car: "alpha-one", paint: "plum" });
  data.set(STORAGE_KEY, "{not json");
  assert.equal(readStored(storage), null);
  const blocked = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); } };
  assert.equal(readStored(blocked), null);
  assert.equal(writeStored(blocked, { car: "a", paint: "b" }), false);
  assert.equal(readStored(undefined), null);
});

test("option rows and labels", () => {
  const catalog = parseCatalog(catalogJson);
  assert.deepEqual(carOptions(catalog), [{ value: "alpha-one", label: "Alpha One" }, { value: "beta-two", label: "Beta Two" }]);
  assert.deepEqual(paintOptions(catalog, "alpha-one").map((o) => o.label), ["Signal Red", "Plum"]);
  assert.equal(paintOptions(catalog, "alpha-one")[0].colour, "#c82622");
  assert.equal(paintOptions(catalog, "missing").length, 2);
  assert.equal(paintLabel("harbour-blue"), "Harbour Blue");
  assert.equal(describeSelection({ car: "a", paint: "b" }), "a:b");
});

test("the prepared field is bounded by the renderer profile limit", () => {
  assert.equal(MAX_FIELD, 8);
  assert.equal(fieldOpponents(3, false), 3);
  assert.equal(fieldOpponents(0, false), 0);
  assert.equal(fieldOpponents(40, false), 7);
  assert.equal(fieldOpponents(-2, false), 0);
  assert.equal(fieldOpponents(3, true), 7);
  assert.equal(fieldOpponents(Number.NaN, false), 7);
});
