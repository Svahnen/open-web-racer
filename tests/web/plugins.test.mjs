import test from "node:test";
import assert from "node:assert/strict";
import { HOOK_NAMES, createPluginRegistry } from "../../web/plugins.js";
import { DEFAULT_PAINT } from "../../web/paint-default.js";

const quiet = () => createPluginRegistry({ warn() {} });

test("register validates ids, hook names and hook shapes", () => {
  const r = quiet();
  assert.throws(() => r.register(null), TypeError);
  assert.throws(() => r.register({ id: "Bad Id" }), TypeError);
  assert.throws(() => r.register({ id: "a", hooks: { nope: {} } }), /unknown hook/);
  assert.throws(() => r.register({ id: "a", hooks: { hud: 3 } }), TypeError);
  assert.throws(() => r.register({ id: "a", hooks: { onRaceEvent: {} } }), TypeError);
  assert.throws(() => r.register({ id: "a", priority: Number.NaN }), TypeError);
  assert.deepEqual(HOOK_NAMES, ["hud", "audio", "sceneSource", "paint", "vehicleModel", "onRaceEvent"]);
  const off = r.register({ id: "a", version: "1.2.3", hooks: { paint: {} } });
  assert.throws(() => r.register({ id: "a" }), /already registered/);
  assert.deepEqual(r.list(), [{ id: "a", version: "1.2.3", priority: 0, hooks: ["paint"] }]);
  off();
  assert.deepEqual(r.list(), []);
});

test("providers order by priority then registration order", () => {
  const r = quiet();
  r.register({ id: "low", priority: -5, hooks: { paint: { n: "low" } } });
  r.register({ id: "first", hooks: { paint: { n: "first" } } });
  r.register({ id: "high", priority: 10, hooks: { paint: { n: "high" } } });
  r.register({ id: "second", hooks: { paint: { n: "second" } } });
  assert.deepEqual(r.providers("paint").map((p) => p.pluginId), ["high", "first", "second", "low"]);
  assert.equal(r.first("paint").n, "high");
  assert.equal(r.first("hud"), null);
});

test("invoke skips throwing and non-answering providers and records failures", () => {
  const r = quiet();
  r.register({ id: "bad", priority: 3, hooks: { vehicleModel: { label() { throw new Error("boom"); } } } });
  r.register({ id: "silent", priority: 2, hooks: { vehicleModel: { label: () => null } } });
  r.register({ id: "none", priority: 1, hooks: { vehicleModel: {} } });
  r.register({ id: "good", priority: 0, hooks: { vehicleModel: { label: (x) => `ok${x}` } } });
  assert.equal(r.invoke("vehicleModel", "label", [7]), "ok7");
  assert.equal(r.invoke("vehicleModel", "missing", [], "fb"), "fb");
  assert.deepEqual(r.failures().map((f) => [f.plugin, f.where, f.message]), [["bad", "vehicleModel.label", "boom"]]);
  assert.deepEqual(r.invokeAll("vehicleModel", "label", [1]), ["ok1"]);
});

test("onRaceEvent reaches every plugin even when one throws or rejects", async () => {
  const r = quiet();
  const seen = [];
  r.register({ id: "a", priority: 2, hooks: { onRaceEvent: (e) => seen.push(["a", e.type]) } });
  r.register({ id: "b", priority: 1, hooks: { onRaceEvent: () => { throw new Error("x"); } } });
  r.register({ id: "c", hooks: { onRaceEvent: async () => { throw new Error("async"); } } });
  r.register({ id: "d", priority: -1, hooks: { onRaceEvent: (e) => seen.push(["d", e.type]) } });
  r.emitRaceEvent({ type: "finish" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(seen, [["a", "finish"], ["d", "finish"]]);
  assert.deepEqual(r.failures().map((f) => f.plugin).sort(), ["b", "c"]);
});

test("loadModules is same-origin only, isolated and honours export shapes", async () => {
  const r = quiet();
  const base = "http://host.test/app/index.html";
  const modules = {
    "http://host.test/p/default.js": { default: () => ({ id: "from-default", hooks: {} }) },
    "http://host.test/p/self.js": { default: (api) => { api.register({ id: "self", hooks: {} }); } },
    "http://host.test/p/named.js": { plugin: { id: "named", hooks: {} } },
    "http://host.test/p/throws.js": null,
    "http://host.test/p/invalid.js": { plugin: { id: "BAD" } },
    "http://host.test/p/hangs.js": "hang",
  };
  const importer = async (href) => {
    const mod = modules[href];
    if (mod === null) throw new Error("import failed");
    if (mod === "hang") return new Promise(() => {});
    return mod;
  };
  const results = await r.loadModules(
    ["/p/throws.js", "http://evil.test/p/x.js", "javascript:alert(1)", "/p/default.js", "/p/invalid.js", "/p/hangs.js", "/p/self.js", "/p/named.js"],
    { base, import: importer, timeoutMs: 20 },
  );
  assert.deepEqual(results.map((x) => x.ok), [false, false, false, true, false, false, true, true]);
  assert.deepEqual(r.list().map((p) => p.id), ["from-default", "self", "named"]);
  assert.equal(r.failures().length, 5);
});

test("configuredUrls merges ?plugins= with the manifest and tolerates a missing manifest", async () => {
  const r = quiet();
  const json = (body) => ({ ok: true, headers: { get: () => "application/json" }, json: async () => body });
  const urls = await r.configuredUrls({
    base: "http://h.test/", search: "?plugins=/a.js, /b.js",
    fetch: async (href) => { assert.equal(href, "http://h.test/plugins.json"); return json({ plugins: ["/b.js", "/c.js", 5] }); },
  });
  assert.deepEqual(urls, ["/a.js", "/b.js", "/c.js"]);
  const none = await r.configuredUrls({ base: "http://h.test/", search: "", fetch: async () => { throw new Error("offline"); } });
  assert.deepEqual(none, []);
  const html = await r.configuredUrls({ base: "http://h.test/", search: "", fetch: async () => ({ ok: true, headers: { get: () => "text/html" }, json: async () => { throw new Error("not json"); } }) });
  assert.deepEqual(html, []);
  const skipped = await r.configuredUrls({ base: "http://h.test/", search: "?plugins=off", fetch: async () => { throw new Error("must not fetch"); } });
  assert.deepEqual(skipped, []);
  const off = await r.configuredUrls({ base: "http://h.test/", search: "?plugins=/a.js", manifestUrl: null });
  assert.deepEqual(off, ["/a.js"]);
});

test("default paint fallback offers a single colour", () => {
  assert.equal(DEFAULT_PAINT.paintChoiceCount(), 1);
  assert.equal(DEFAULT_PAINT.paintColourFromId("paint-colour-0"), 0);
  assert.equal(DEFAULT_PAINT.paintColourFromId("paint-colour-3"), null);
  const registered = new Set([DEFAULT_PAINT.variantKey("p", "f", 0)]);
  assert.equal(DEFAULT_PAINT.resolveRenderColour(registered, "p", "f", 5), 0);
  assert.equal(DEFAULT_PAINT.resolveRenderColour(registered, "q", "f", 0), undefined);
});
