// Browser plugin registry (see docs/PLUGINS.md).
//
// A plugin is a plain object `{ id, version, hooks, priority? }`. Hooks are
// optional, named capabilities the page consults instead of hard-coding
// content-specific behaviour. Every call into plugin code is isolated: a throwing
// or rejecting plugin is recorded in `failures()` and skipped, never fatal.
//
//   window.RacerPlugins.register({
//     id: "example", version: "1.0.0",
//     hooks: { paint: { ... }, onRaceEvent(event) { ... } },
//   });
//
// Ordering: higher `priority` first (default 0); ties keep registration order.

export const HOOK_NAMES = Object.freeze(["hud", "audio", "sceneSource", "paint", "vehicleModel", "onRaceEvent"]);

const ID_PATTERN = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const DEFAULT_LOAD_TIMEOUT_MS = 8000;

export function createPluginRegistry(options = {}) {
  const log = options.warn ?? ((...args) => console.warn(...args));
  const plugins = new Map();
  const failureLog = [];
  let sequence = 0;

  function recordFailure(pluginId, where, error) {
    const message = error instanceof Error ? error.message : String(error);
    failureLog.push({ plugin: pluginId, where, message });
    if (failureLog.length > 100) failureLog.shift();
    log(`plugin ${pluginId}: ${where} failed: ${message}`);
  }

  function register(plugin) {
    if (!plugin || typeof plugin !== "object") throw new TypeError("plugin must be an object");
    if (typeof plugin.id !== "string" || !ID_PATTERN.test(plugin.id)) {
      throw new TypeError("plugin.id must be 1-64 characters of a-z 0-9 . _ -");
    }
    if (plugin.version !== undefined && typeof plugin.version !== "string") {
      throw new TypeError("plugin.version must be a string");
    }
    if (plugin.priority !== undefined && !Number.isFinite(plugin.priority)) {
      throw new TypeError("plugin.priority must be a finite number");
    }
    const hooks = plugin.hooks ?? {};
    if (typeof hooks !== "object" || hooks === null) throw new TypeError("plugin.hooks must be an object");
    for (const [name, value] of Object.entries(hooks)) {
      if (!HOOK_NAMES.includes(name)) throw new TypeError(`unknown hook "${name}"`);
      if (name === "onRaceEvent") {
        if (typeof value !== "function") throw new TypeError("hooks.onRaceEvent must be a function");
      } else if (!value || typeof value !== "object") {
        throw new TypeError(`hooks.${name} must be an object`);
      }
    }
    if (plugins.has(plugin.id)) throw new Error(`plugin "${plugin.id}" is already registered`);
    const entry = { id: plugin.id, version: plugin.version ?? "0.0.0", priority: plugin.priority ?? 0, hooks: { ...hooks }, order: sequence++ };
    plugins.set(entry.id, entry);
    return () => {
      if (plugins.get(entry.id) === entry) plugins.delete(entry.id);
    };
  }

  function ordered() {
    return [...plugins.values()].sort((a, b) => b.priority - a.priority || a.order - b.order);
  }

  /** `[{ id, version, priority, hooks: [names] }]` in call order. */
  function list() {
    return ordered().map(({ id, version, priority, hooks }) => ({ id, version, priority, hooks: Object.keys(hooks) }));
  }

  /** `[{ pluginId, hook }]` for every plugin providing `name`, in call order. */
  function providers(name) {
    return ordered().filter((entry) => entry.hooks[name] !== undefined).map((entry) => ({ pluginId: entry.id, hook: entry.hooks[name] }));
  }

  /** The highest-priority hook object for `name`, or null. */
  function first(name) {
    return providers(name)[0]?.hook ?? null;
  }

  /**
   * Calls `method(...args)` on providers of `name` in order and returns the
   * first non-nullish result. A provider that throws is recorded and skipped.
   * Returns `fallback` when no provider answers.
   */
  function invoke(name, method, args = [], fallback = undefined) {
    for (const { pluginId, hook } of providers(name)) {
      if (typeof hook[method] !== "function") continue;
      try {
        const result = hook[method](...args);
        if (result !== undefined && result !== null) return result;
      } catch (error) {
        recordFailure(pluginId, `${name}.${method}`, error);
      }
    }
    return fallback;
  }

  /** Calls `method` on every provider (in order); returns the non-nullish results. */
  function invokeAll(name, method, args = []) {
    const results = [];
    for (const { pluginId, hook } of providers(name)) {
      if (typeof hook[method] !== "function") continue;
      try {
        const result = hook[method](...args);
        if (result !== undefined && result !== null) results.push(result);
      } catch (error) {
        recordFailure(pluginId, `${name}.${method}`, error);
      }
    }
    return results;
  }

  /** Delivers a race event `{ type, ... }` to every `onRaceEvent` hook. */
  function emitRaceEvent(event) {
    for (const { pluginId, hook } of providers("onRaceEvent")) {
      try {
        const result = hook(event);
        if (result && typeof result.catch === "function") result.catch((error) => recordFailure(pluginId, "onRaceEvent", error));
      } catch (error) {
        recordFailure(pluginId, "onRaceEvent", error);
      }
    }
  }

  function failures() {
    return failureLog.map((entry) => ({ ...entry }));
  }

  /** Resolves `url` against `base`; returns the absolute URL only for same-origin http(s). */
  function sameOriginUrl(url, base) {
    let parsed;
    try {
      parsed = new URL(url, base);
    } catch {
      return null;
    }
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    if (parsed.origin !== new URL(base).origin) return null;
    return parsed.href;
  }

  /**
   * Loads plugin ES modules. A module either calls `register` on the global
   * registry itself, default-exports `(registry) => void | plugin object`, or
   * exports a `plugin` object. Each URL is isolated and time limited.
   */
  async function loadModules(urls, loadOptions = {}) {
    const base = loadOptions.base ?? (typeof location !== "undefined" ? location.href : "http://localhost/");
    const importer = loadOptions.import ?? ((href) => import(/* @vite-ignore */ href));
    const timeoutMs = loadOptions.timeoutMs ?? DEFAULT_LOAD_TIMEOUT_MS;
    const results = [];
    for (const url of urls) {
      const href = sameOriginUrl(url, base);
      if (!href) {
        recordFailure(String(url), "load", new Error("plugin URL must be same-origin http(s)"));
        results.push({ url, ok: false });
        continue;
      }
      let timer;
      try {
        const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("timed out")), timeoutMs); });
        const work = (async () => {
          const mod = await importer(href);
          let plugin = mod?.plugin;
          if (typeof mod?.default === "function") plugin = (await mod.default(api)) ?? plugin;
          else if (mod?.default && typeof mod.default === "object") plugin = mod.default;
          if (plugin && typeof plugin === "object") register(plugin);
        })();
        work.catch(() => {});
        await Promise.race([work, timeout]);
        results.push({ url: href, ok: true });
      } catch (error) {
        recordFailure(href, "load", error);
        results.push({ url: href, ok: false });
      } finally {
        clearTimeout(timer);
      }
    }
    return results;
  }

  /** Plugin URLs from `?plugins=a.js,b.js` plus the optional same-origin manifest `{ "plugins": [urls] }`. */
  async function configuredUrls(configOptions = {}) {
    const base = configOptions.base ?? (typeof location !== "undefined" ? location.href : "http://localhost/");
    const search = configOptions.search ?? (typeof location !== "undefined" ? location.search : "");
    const fetcher = configOptions.fetch ?? (typeof fetch === "function" ? fetch : null);
    const urls = [];
    const param = new URLSearchParams(search).get("plugins");
    if (param === "off") return [];
    if (param) urls.push(...param.split(",").map((part) => part.trim()).filter(Boolean));
    const manifestUrl = configOptions.manifestUrl === undefined ? "/plugins.json" : configOptions.manifestUrl;
    if (manifestUrl && fetcher) {
      try {
        const href = sameOriginUrl(manifestUrl, base);
        if (href) {
          const response = await fetcher(href, { cache: "no-store" });
          if (response.ok && /json/i.test(response.headers?.get?.("content-type") ?? "json")) {
            const body = await response.json();
            if (Array.isArray(body?.plugins)) urls.push(...body.plugins.filter((item) => typeof item === "string"));
          }
        }
      } catch {
        // The manifest is optional.
      }
    }
    return [...new Set(urls)];
  }

  async function loadConfigured(configOptions = {}) {
    const urls = await configuredUrls(configOptions);
    return loadModules(urls, configOptions);
  }

  const api = { register, list, providers, first, invoke, invokeAll, emitRaceEvent, failures, loadModules, configuredUrls, loadConfigured };
  return api;
}

export const RacerPlugins = createPluginRegistry();

if (typeof window !== "undefined" && !window.RacerPlugins) {
  window.RacerPlugins = RacerPlugins;
}
