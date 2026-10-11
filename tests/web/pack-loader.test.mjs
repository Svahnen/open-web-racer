import test from "node:test";
import assert from "node:assert/strict";
import { createPackLoaderRegistry } from "../../web/pack-loader.js";

const pack = { manifest: { schema: "rcp/1", id: "x" } };
const files = [{ path: "pack.json", size: 2, read: async () => new ArrayBuffer(2) }];

function loader(id, overrides = {}) {
  return {
    id,
    detect: (f) => f.some((x) => x.path === "pack.json"),
    load: async () => pack,
    ...overrides,
  };
}

test("register validates loader shape and rejects duplicates", () => {
  const r = createPackLoaderRegistry();
  assert.throws(() => r.register(null), TypeError);
  assert.throws(() => r.register(loader("Bad Id")), TypeError);
  assert.throws(() => r.register({ id: "a", detect() {} }), TypeError);
  const off = r.register(loader("a"));
  assert.throws(() => r.register(loader("a")), /already registered/);
  assert.deepEqual(r.list(), ["a"]);
  off();
  assert.deepEqual(r.list(), []);
});

test("detect tolerates throwing loaders and load dispatches to the first match", async () => {
  const r = createPackLoaderRegistry();
  r.register(loader("broken", { detect() { throw new Error("boom"); } }));
  r.register(loader("native"));
  assert.deepEqual(r.detect(files), ["native"]);
  assert.equal(await r.load(files), pack);
  assert.equal(await r.load(files, { loader: "native" }), pack);
  await assert.rejects(r.load(files, { loader: "missing" }), /not registered/);
});

test("load rejects when nothing matches or the result is not an rcp/1 pack", async () => {
  const r = createPackLoaderRegistry();
  await assert.rejects(r.load(files), /no registered loader/);
  r.register(loader("bad", { load: async () => ({ manifest: { schema: "other" } }) }));
  await assert.rejects(r.load(files), /did not return an rcp\/1 pack/);
});
