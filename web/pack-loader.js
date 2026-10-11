// Content pack loader registry (plugin API).
//
// A loader turns a set of user-selected files into an RCP v1 pack object (see
// docs/CONTENT_PACK.md). This module only keeps the registry; it is not wired
// into any import UI yet.
//
//   window.RacerPackLoader.register({
//     id: "my-loader",
//     detect(files) { return files.some((f) => f.path === "pack.json"); },
//     async load(ctx) { return { manifest, tracks, cars, vehicles, audio, hud, buffers }; },
//   });
//
// `files` is an array of `{ path, size, read(): Promise<ArrayBuffer> }` with
// forward-slash relative paths. `detect` is synchronous and must be cheap and
// side-effect free. `load` receives `ctx = { files, signal, progress(fraction, label) }`
// and resolves to a pack object; it must not upload or persist the input.

export function createPackLoaderRegistry() {
  const loaders = new Map();

  function register(loader) {
    if (!loader || typeof loader !== "object") {
      throw new TypeError("loader must be an object");
    }
    if (typeof loader.id !== "string" || !/^[a-z0-9][a-z0-9._-]{0,63}$/.test(loader.id)) {
      throw new TypeError("loader.id must be 1-64 characters of a-z 0-9 . _ -");
    }
    if (typeof loader.detect !== "function" || typeof loader.load !== "function") {
      throw new TypeError("loader.detect and loader.load must be functions");
    }
    if (loaders.has(loader.id)) {
      throw new Error(`loader "${loader.id}" is already registered`);
    }
    loaders.set(loader.id, loader);
    return () => {
      if (loaders.get(loader.id) === loader) loaders.delete(loader.id);
    };
  }

  function list() {
    return [...loaders.keys()];
  }

  function detect(files) {
    const matches = [];
    for (const loader of loaders.values()) {
      let ok = false;
      try {
        ok = Boolean(loader.detect(files));
      } catch {
        ok = false;
      }
      if (ok) matches.push(loader.id);
    }
    return matches;
  }

  async function load(files, options = {}) {
    const id = options.loader ?? detect(files)[0];
    if (!id) throw new Error("no registered loader recognised these files");
    const loader = loaders.get(id);
    if (!loader) throw new Error(`loader "${id}" is not registered`);
    const ctx = {
      files,
      signal: options.signal,
      progress: options.progress ?? (() => {}),
    };
    const pack = await loader.load(ctx);
    if (!pack || pack.manifest?.schema !== "rcp/1") {
      throw new Error(`loader "${id}" did not return an rcp/1 pack`);
    }
    return pack;
  }

  return { register, list, detect, load };
}

export const RacerPackLoader = createPackLoaderRegistry();

if (typeof window !== "undefined" && !window.RacerPackLoader) {
  window.RacerPackLoader = RacerPackLoader;
}
