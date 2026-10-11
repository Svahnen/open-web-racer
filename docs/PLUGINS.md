# Browser plugins

`web/plugins.js` exposes `window.RacerPlugins`, a small registry that lets optional code supply content-specific behaviour (HUD, sounds, local-file import, car colours, handling-model parameters) without the core page knowing about it. It sits beside `window.RacerPackLoader` (content packs, see `docs/CONTENT_PACK.md`).

The public page ships no plugin. A build that needs one (for example a private package that imports local game files) serves it from a directory (see "Serving" below) and the page loads it at start-up. Without a plugin the page loads and races project tracks and cars normally; plugin-only UI (such as a local-file import) simply does not exist.

## Registering

```js
window.RacerPlugins.register({
  id: "my-plugin",        // 1-64 chars of a-z 0-9 . _ -
  version: "1.0.0",       // informational
  priority: 0,            // optional; higher runs first
  hooks: { paint: { /* ... */ }, onRaceEvent(event) { /* ... */ } },
});
```

`register` returns an unregister function. Duplicate ids, unknown hook names and malformed hooks throw `TypeError`/`Error` at registration time.

Ordering: providers are consulted by descending `priority`, ties in registration order. Single-answer hooks use the first provider that implements the method and returns a non-null value, so a higher-priority plugin overrides lower ones per method.

Isolation: every call into plugin code is wrapped. A plugin that throws, rejects or times out is recorded (`RacerPlugins.failures()`) and skipped; the page keeps working with the next provider or its default.

## Hooks

All hook values are objects (except `onRaceEvent`). Every member is optional; a missing member means "not provided".

- `hud`: replaces the race HUD. Members: `createRenderer({canvas, routeCanvas})` returning `{render(model, extras), setArt(art), artKind}`; `createLapTracker()`; `createWrongWayDetector()`; `buildModel(frame)` producing the renderer's model; `toTicks(sim60hzTicks)`; `classifyPath(relativePath)` returning a key for files the plugin wants from a selected folder, or null; `loadArt({read(name), wasm, isCurrent()})` resolving art or null; `artSpeedUnit` preferred speed unit when art loads. The page registers its own arcade-style HUD (`web/arcade-hud.js`, plugin id `owr-arcade-hud`, priority -1000, `builtin: true`) as the default `hud` provider, so any plugin that provides `hud` overrides it. `?hud=diag` leaves the built-in HUD out and shows the diagnostic card HUD instead. The page passes extra frame fields (`transmission`, `countdownTicks`, `sinceStartTicks`, `finished`, `results`, `laps`) to `buildModel` and extra render inputs (`trackMap`, `player`, `rivalPoses`) to `render`; a plugin may ignore them.
- `audio`: sampled engine and voice sources. Members: `createVoices({ctx, library, buses})` returning an object with `setCar, updateEngine, updateSkid, setHorn, updateRemotes, playImpact, silence, dispose, active`; `noteCandidate(map, relativePath, file)` to collect candidate files while a folder is scanned; `stemFromPath(path)`; `decoder(wasm)`; `discover({candidates, stems, decoder})` resolving a library; `statusText(library, stem)`. The page hands a discovered library to `GameAudio.setSampleLibrary(library, carSpec)`. Without a provider the synthesized sounds are used.
- `sceneSource`: a source of scenes the page cannot generate itself, typically a local-file import. Members:
  - `id`, `label`, `accepts: "directory"|"files"`: names the entry.
  - `mount(host)`: called once at page start-up (before the renderer exists; use `host.wasm`, `host.race` and `host.gpu` lazily). The plugin builds its own UI in the page's slots (`#scene-source-practice` in the solo session panel, `#scene-source-lobby` in the lobby panel, `#scene-source-panel` below the game), injects its own styles, and returns a controller. Anything other than an object is treated as "no source".
  - `contentCapabilities`: extra strings appended to the `Hello` capability list.
  - `contentIdentity`: content identity string the source's rooms use.
  - `privateMode`: `{ localLabel, networkLabel, title, description }`. `?scene=private` (a scene served by a provider the source knows) only has an effect when a source declares this; the page then shows these texts.
  - `onServerMessage(kind, body, host)`: receives server messages the page does not handle itself; return true when consumed.
- `paint`: car colour tables. Members: `paintIdForColour, paintColourFromId, paintChoiceCount(vehicleData), clampPaintColour, paintChoiceOptions, paintTableBytes, carArchiveAPath, variantKey, resolveRenderColour, requiredPaintVariants`. `vehicleData` is the opaque block a scene source stores in `host.vehicleData`. Without a provider `web/paint-default.js` offers one colour.
- `vehicleModel`: handling-model integration. Members: `handlingLabel(id)` label for a protocol handling-model id (ids the core does not know); `message({hostname, race})` returning a client message JSON string to send for the local car (sent on connect, on room create/join and when the scene changes), or null; `modelFor(selection)` (reserved) returning `{model_id, params}` for a `SetCarModel` message (protocol 11) for the selected car.
- `onRaceEvent(event)`: called for race lifecycle changes. `event = {type: "session"|"status"|"finish", session, status, finished, connected, lap}`. Return values are ignored; promises that reject are recorded.

### Scene source controller

`mount(host)` returns an object the page calls. Every member is optional:

- `loading` (boolean): an import is running; room pickers are disabled meanwhile.
- `refresh()`: re-render the source's own pickers (room membership or selection changed).
- `restoreSaved()`: awaited once after the renderer is up (not in provider mode); restores a previously cached scene.
- `revealProviderEntry()` / `loadProviderScene()`: reveal the entry for a provider the source knows / load that scene (`?scene=private`).
- `syncRoomTemplates()`: resolves the list of vehicle templates the room needs prepared locally (empty list when none). The page waits for it before it lets the player ready up in rooms that need local templates.
- `selectionForTrack(profileId, availableTracks)`: join/create selection `{ok, content_identity, track_profile_id, track_fingerprint, vehicle_profile_id, vehicle_fingerprint}` or `{ok: false, reason}` for a track profile only the source understands.
- `trackLabel(profileId)`, `roomTrackLabel(profileId)`, `vehicleLabel(profileId)`: menu texts; the id is shown when missing.
- `paintPicked(colour)`: the player picked a colour in one of the page's pickers.
- `vehicleStem(profileId)`: key for the sampled-voice bank of a car profile.
- `notice(text)`: show a status line.

### Scene source host

`host` is a context object. Read-only accessors: `race`, `wasm`, `gpu`, `playerId`, `activeRaceId`, `connected`, `networkSessionRevision`, `authoritative`. Read/write accessors (the page and the source share this state): `simulationSpawnPose`, `privateScenePose`, `privateSceneRevision`, `previewOpenDrive`, `previewGrounded`, `registeredVehicleProfiles`, `registeredTrackFingerprint`, `activeSceneTrackFingerprint`, `roomSelectionRevision`, the `roomTemplate*` synchronisation fields, `currentRoomTemplateProfiles`, `paintColour`, `scenePaintColour`, `vehicleData`. Plain members: `statusEl`, `lapEl`, `tickEl`, `localPracticeMode`, `gameAudio`, `networkPaintChoice`, `hud` (the canvas HUD renderer), `paint.effective()`, `paint.populate(select, colour)`, `roomSummary()`, `captureRoomWorkToken()`, `isCurrentRoomWorkToken(token)`, `currentRoomWorkState()`, `ifCurrentRoomWork(token, state, action)`, `applyRouteMap(descriptor)`, `sendVehicleModelMessage()`, `updateNetworkTrackChoice()`, `setSpeedUnit(unit)`.

## Loading

Plugins load as ES modules from same-origin http(s) URLs only (cross-origin, `data:`, `javascript:` and similar are refused), each with an 8 second limit, one at a time in list order, before the page starts:

- `?plugins=/p/a.js,/p/b.js` in the page URL (`?plugins=off` loads nothing, not even the manifest);
- an optional same-origin `/plugins.json`: `{ "plugins": ["/p/a.js"] }` (a missing or non-JSON response is ignored).

A module may call `window.RacerPlugins.register(...)` itself, `export default (registry) => plugin | void`, or `export const plugin = {...}`.

## Serving

The server serves one plugin directory when `RACER_PLUGIN_DIR` is set (or `ServerConfig::plugin_dir` is called):

- the directory is mounted read-only at `/plugins/ext/`;
- `/plugins.json` advertises the entry modules. The directory's `plugin.json` (`{ "plugins": ["main.js"] }`, names relative to the directory, `.js`/`.mjs` only, no `..`) lists them; without that file `index.js` is used when it exists;
- when the variable is unset, or the directory is not valid, nothing is served and `/plugins.json` is a plain 404.

Plugin modules may import each other by relative path but must not import page modules by relative path: the page serves them from a different place than the repository layout. A plugin needing page helpers receives them through `host`.

Copy a plugin package next to `web/` (for example as `plugin/`) and start the server with `RACER_PLUGIN_DIR` pointing at it. Without the variable no plugin is served.

## Testing

`tests/web/plugins.test.mjs` covers validation, ordering, failure isolation, race events, module loading and manifest handling using `createPluginRegistry()`, which builds independent registries. `crates/server/tests/plugin_dir.rs` covers serving. Tests of a separate plugin package live next to it.
