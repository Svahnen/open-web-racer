// Selection of the project-owned demo cars. No DOM here; unit tested.
//
// The catalogue comes from the renderer build (`project_car_catalog()`):
// `{ cars: [{ id, name, paints: [{ id, body }] }] }`.

export const STORAGE_KEY = "open-web-racer.project-car";
export const MAX_FIELD = 8;

const ID = /^[a-z0-9][a-z0-9-]{0,47}$/;

/** Parse and validate the catalogue JSON; throws on a malformed catalogue. */
export function parseCatalog(json) {
  const raw = typeof json === "string" ? JSON.parse(json) : json;
  if (!raw || !Array.isArray(raw.cars) || raw.cars.length === 0) throw new Error("project car catalogue is empty");
  const seen = new Set();
  const cars = raw.cars.map((car) => {
    if (typeof car?.id !== "string" || !ID.test(car.id) || seen.has(car.id)) throw new Error("project car id is invalid");
    seen.add(car.id);
    if (typeof car.name !== "string" || car.name.trim() === "") throw new Error("project car name is invalid");
    if (!Array.isArray(car.paints) || car.paints.length === 0) throw new Error("project car has no paints");
    const paints = car.paints.map((paint) => {
      if (typeof paint?.id !== "string" || !ID.test(paint.id)) throw new Error("project paint id is invalid");
      return { id: paint.id, name: paintLabel(paint.id), body: /^#[0-9a-f]{6}$/.test(paint.body) ? paint.body : "#808080" };
    });
    return { id: car.id, name: car.name, paints, engineSound: car.engine_sound && typeof car.engine_sound === "object" ? car.engine_sound : null };
  });
  return { cars };
}

/** `signal-red` becomes `Signal Red`. */
export function paintLabel(id) {
  return String(id).split("-").map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ");
}

export function defaultSelection(catalog) {
  return { car: catalog.cars[0].id, paint: catalog.cars[0].paints[0].id };
}

/** Force a requested pair onto a car and paint that exist. */
export function resolveSelection(catalog, car, paint) {
  const fallback = defaultSelection(catalog);
  const entry = catalog.cars.find((candidate) => candidate.id === car);
  if (!entry) return fallback;
  const paintEntry = entry.paints.find((candidate) => candidate.id === paint) ?? entry.paints[0];
  return { car: entry.id, paint: paintEntry.id };
}

/** Selection from `?car=&paint=`, else the stored one, else the default. */
export function selectionFromSources(catalog, search, stored) {
  const params = new URLSearchParams(search);
  if (params.has("car")) return resolveSelection(catalog, params.get("car"), params.get("paint"));
  if (stored && typeof stored === "object") return resolveSelection(catalog, stored.car, stored.paint);
  return defaultSelection(catalog);
}

export function readStored(storage) {
  try {
    const value = JSON.parse(storage?.getItem(STORAGE_KEY) ?? "null");
    return value && typeof value.car === "string" ? value : null;
  } catch {
    return null;
  }
}

export function writeStored(storage, selection) {
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(selection));
    return true;
  } catch {
    return false;
  }
}

/** Option rows for the car select. */
export function carOptions(catalog) {
  return catalog.cars.map((car) => ({ value: car.id, label: car.name }));
}

/** Option rows for the paint select of one car. */
export function paintOptions(catalog, carId) {
  const car = catalog.cars.find((candidate) => candidate.id === carId) ?? catalog.cars[0];
  return car.paints.map((paint) => ({ value: paint.id, label: paint.name, colour: paint.body }));
}

/** How many opponent cars to prepare: solo uses its set-up, rooms the maximum. */
export function fieldOpponents(soloCount, inRoom) {
  if (inRoom || !Number.isFinite(soloCount)) return MAX_FIELD - 1;
  return Math.min(MAX_FIELD - 1, Math.max(0, Math.trunc(soloCount)));
}

/** Compact description published on the page for tests and diagnostics. */
export function describeSelection(selection) {
  return `${selection.car}:${selection.paint}`;
}

/** Marks "no project car" in the packed pick list handed to the renderer. */
export const NO_CAR = 255;

/** Car and paint indices (catalogue order) of a selection, or null. */
export function selectionIndices(catalog, selection) {
  const car = catalog.cars.findIndex((candidate) => candidate.id === selection?.car);
  if (car < 0) return null;
  const paint = catalog.cars[car].paints.findIndex((candidate) => candidate.id === selection.paint);
  return paint < 0 ? [car, 0] : [car, paint];
}

/** Indices of a wire choice (model id and paint scheme index), or null. */
export function wireIndices(catalog, model, variant) {
  if (typeof model !== "string" || model === "") return null;
  const car = catalog.cars.findIndex((candidate) => candidate.id === model);
  if (car < 0 || !Number.isInteger(variant) || variant < 0 || variant >= catalog.cars[car].paints.length) return null;
  return [car, variant];
}

/** Wire fields (`car_model`, `car_variant`) that announce a selection to a room. */
export function wireFields(catalog, selection) {
  const indices = catalog ? selectionIndices(catalog, selection) : null;
  if (!indices) return { car_model: "", car_variant: 0 };
  return { car_model: catalog.cars[indices[0]].id, car_variant: indices[1] };
}

/** Flat `car, paint` byte pairs for the renderer; null entries become [NO_CAR, NO_CAR]. */
export function packPicks(indexPairs) {
  const bytes = new Uint8Array(indexPairs.length * 2);
  indexPairs.forEach((pair, index) => {
    bytes[index * 2] = pair ? pair[0] : NO_CAR;
    bytes[index * 2 + 1] = pair ? pair[1] : NO_CAR;
  });
  return bytes;
}

/** Entry `slot` of the flat roster bytes (driver = slot 0), or null. */
export function rosterPair(roster, slot) {
  if (!roster || roster.length < slot * 2 + 2) return null;
  return [roster[slot * 2], roster[slot * 2 + 1]];
}
