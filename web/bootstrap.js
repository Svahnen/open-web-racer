import { buildHudModel, controlsHintFor, convertSpeed, createGearHintTracker, createRouteMapView, createSessionClock, gearKind, setRouteMapFromSceneDescriptor } from "./hud-model.js";
import { buildTrackMap, registerArcadeHud } from "./arcade-hud.js";
import { createChaseCameraSmoother, createFrameFaultHandler, createFrameLoop } from "./frame-loop.js";
import { RacerPlugins } from "./plugins.js";
import { DEFAULT_PAINT } from "./paint-default.js";
import { mirrorVisibilityAfterKeydown } from "./mirror-controls.js";
import { requireMirrorVisibilityApi } from "./runtime-api.js";
import { readSoundPreference, readSpeedUnit, soundPreferenceLocation, speedUnitPreferenceLocation } from "./display-preferences.js";
import { GameAudio, lateralSpeed, skidLevel } from "./audio.js";
import { ImpactDetector, remoteCarsForAudio } from "./audio-feed.js";
import { createLobbyCloseHandler, ifCurrentRoomWork, isCurrentRaceMessage, isCurrentRoomSnapshot, isCurrentRoomWork, matchesReadyAcknowledgement, reconcileSelectOptions, roomConnectionUiState } from "./multiplayer-state.js";
import { createShiftLatch, InputHistory } from "./input-history.js";
import { PROJECT_TRACK_CONTENT_ID, parseCatalog, parseTrackParam, projectTrackIdFromProfile, projectTrackPose, TEST_CIRCUIT_TRACK_ID, resolveCatalogEntry, soloTrackHref } from "./project-tracks.js";
import { clampTransmission, isManual, parseTransmissionParam, setTransmissionMessage, transmissionSummary } from "./transmission-setting.js";
import { DEFAULT_OPPONENTS, clampLaps, clampOpponents, clampSkill, decodeOpponentStates, opponentSummary, opponentVoices, parseOpponentParams, projectRenderPoses, soloQuery } from "./opponent-settings.js";
import { carOptions, describeSelection, fieldOpponents, packPicks, paintOptions, parseCatalog as parseProjectCatalog, readStored as readStoredProjectCar, resolveSelection as resolveProjectSelection, rosterPair, selectionFromSources, selectionIndices, wireFields, wireIndices, writeStored as writeStoredProjectCar } from "./project-cars.js";

// Plugin hooks (docs/PLUGINS.md). Plugins load from `?plugins=` or the same-origin
// /plugins.json (`?plugins=off` skips them). Load failures are isolated and never fatal.
// The built-in canvas HUD is the lowest-priority `hud` provider, so a plugin's HUD overrides it;
// `?hud=diag` leaves it out and shows the diagnostic card HUD instead.
if (new URLSearchParams(location.search).get("hud") !== "diag") registerArcadeHud(RacerPlugins);
await RacerPlugins.loadConfigured();
const hudHook = RacerPlugins.first("hud");
const sceneSourceHook = RacerPlugins.first("sceneSource");
// `?scene=private` selects the scene source's own provider mode; it needs a plugin that offers one.
const privateSceneRequested = new URLSearchParams(location.search).get("scene") === "private" && !!sceneSourceHook?.privateMode;
const paintFn = (name) => (...args) => (RacerPlugins.first("paint")?.[name] ?? DEFAULT_PAINT[name])(...args);
const clampPaintColour = paintFn("clampPaintColour");
const paintChoiceCount = paintFn("paintChoiceCount");
const paintChoiceOptions = paintFn("paintChoiceOptions");
const paintColourFromId = paintFn("paintColourFromId");
const paintIdForColour = paintFn("paintIdForColour");
const resolveRenderColour = paintFn("resolveRenderColour");
const toHudTicks = (ticks) => (hudHook?.toTicks ? hudHook.toTicks(ticks) : ticks);


const gpuCanvas = document.querySelector("#race-canvas");
const webglCanvas = document.querySelector("#webgl-canvas");
const fallbackCanvas = document.querySelector("#fallback-canvas");
let canvas = gpuCanvas;
let ctx = null;
const statusEl = document.querySelector("#engine-status");
const speedEl = document.querySelector("#speed");
const speedUnitEl = document.querySelector("#speed-unit");
const gearSlotEl = document.querySelector("#gear-slot");
const gearEl = document.querySelector("#gear");
const gearHintEl = document.querySelector("#gear-hint");
const autoReverseInput = document.querySelector("#auto-reverse");
// Gearbox mode of the local driver: from the solo link in practice, from the
// lobby selector in a room. The controls hint follows it.
let networkTransmission = "automatic";
let soloTransmission = "automatic";
// Auto reverse: the simulation (local and server) never needs a shift; brake at
// a standstill reverses, throttle in R brakes and then drives forward again.
let autoReverseEnabled = false;
try {
  autoReverseEnabled = new URLSearchParams(location.search).get("autoreverse") === "1"
    || window.localStorage.getItem("open-web-racer.autoReverse") === "1";
} catch { /* storage may be blocked */ }
function activeTransmission() { return localPracticeModeEarly ? soloTransmission : networkTransmission; }
function refreshControlsHint() {
  for (const element of document.querySelectorAll("[data-controls-hint]")) element.textContent = controlsHintFor(activeTransmission(), autoReverseEnabled);
}
const localPracticeModeEarly = new URLSearchParams(location.search).get("practice") === "local";
if (localPracticeModeEarly) soloTransmission = parseTransmissionParam(location.search);
refreshControlsHint();
const lapEl = document.querySelector("#lap");
const lapCardEl = document.querySelector("#hud-lap-card");
const lapTargetEl = document.querySelector("#lap-target");
const hudTimeLabelEl = document.querySelector("#hud-time-label");
const hudTimeEl = document.querySelector("#hud-time");
const hudStateEl = document.querySelector("#hud-state");
const hudEl = document.querySelector("#game-hud");
const hudRaceEl = document.querySelector("#hud-race");
const hudPositionEl = document.querySelector("#hud-position");
const hudLapTimeLabelEl = document.querySelector("#hud-lap-time-label");
const hudLapTimeEl = document.querySelector("#hud-lap-time");
const hudLastLapEl = document.querySelector("#hud-last-lap");
const hudBestLapEl = document.querySelector("#hud-best-lap");
const gameEl = document.querySelector(".game");
const routeMapPanel = document.querySelector("#route-map-panel");
const routeMapCanvas = document.querySelector("#route-map");
const routeMapView = createRouteMapView({ panel: routeMapPanel, canvas: routeMapCanvas, game: gameEl });
const tickEl = document.querySelector("#tick");
const toast = document.querySelector("#toast");
const keys = new Set();
let race = null;
let simulationSpawnPose = null;
let wasmModule = null;
let gpu = false;
let previewMirrorVisible = true;
let fallback = { x: -20, y: 0, heading: 0, vx: 0, vy: 0, lap: 1, checkpoint: 1, tick: 0 };
let frameFaulted = false;
let socket = null;
let playerId = null;
let activeRaceId = null;
let connected = false;
let networkSessionRevision = 0;
let authoritative = null;
let latestLobby = [];
let latestAvailableTracks = [];
let readyState = false;
let pendingReadyRequest = null;
let roomSelectionRevision = 0;
let roomTemplateSyncSignature = null;
let roomTemplateSyncPromise = Promise.resolve();
let roomTemplateSyncError = null;
let roomTemplateFailedSignature = null;
let roomTemplateSyncEpoch = 0;
let roomTemplateReadySignature = null;
let registeredVehicleProfiles = new Set();
// Car paint: the chosen colour index (null = scene default), the opaque vehicle-data
// block a scene source may supply (it sizes the colour pickers) and the colour
// baked into the active scene.
let paintColour = null;
let vehicleData = null;
let scenePaintColour = null;
let registeredTrackFingerprint = null;
let activeSceneTrackFingerprint = null;
let currentRoomTemplateProfiles = [];
let roomRosterEl = null;
const inputHistory = new InputHistory();
let networkElapsed = 0;
let privateScenePose = null;
let privateSceneRevision = 0;
const hudSessionClock = createSessionClock();
// Canvas HUD from the `hud` plugin hook; without one the diagnostic card HUD stays visible.
const hudCanvas = document.querySelector("#hud-canvas");
const canvasHud = hudHook?.createRenderer?.({ canvas: hudCanvas, routeCanvas: routeMapCanvas }) ?? { setArt() {}, render() {}, artKind: "none" };
if (!hudHook?.createRenderer) hudCanvas.hidden = true;
const hudLapTracker = hudHook?.createLapTracker?.() ?? null;
const hudWrongWay = hudHook?.createWrongWayDetector?.() ?? null;
let hudRoutePath = null;
let hudRivals = [];
// Mini map and wrong-way path of the built-in HUD: a generated track's outline, or the test circuit's.
let hudTrackMap = null;
let hudCircuitMap;
let hudMapInUse = null;
let hudActors = { player: null, rivalPoses: [] };
function currentHudMap() {
  if (activeProjectTrack) return hudTrackMap;
  if (privateScenePose) return null;
  if (hudCircuitMap === undefined) {
    try { hudCircuitMap = buildTrackMap(JSON.parse(wasmModule.project_circuit_map_json())); } catch { hudCircuitMap = null; }
  }
  return hudCircuitMap;
}
let hudContext = null;
const hudDiagnosticsInput = document.querySelector("#hud-diagnostics");
const hudDiagnosticsOn = new URLSearchParams(location.search).get("hud") === "diag";
gameEl.classList.toggle("hud-diagnostics", hudDiagnosticsOn || !hudHook?.createRenderer);
if (hudDiagnosticsInput) {
  hudDiagnosticsInput.checked = hudDiagnosticsOn;
  hudDiagnosticsInput.addEventListener("change", () => {
    gameEl.classList.toggle("hud-diagnostics", hudDiagnosticsInput.checked || !hudHook?.createRenderer);
    if (hudHook?.builtin) hudCanvas.hidden = hudDiagnosticsInput.checked;
    const url = new URL(location.href);
    if (hudDiagnosticsInput.checked) url.searchParams.set("hud", "diag"); else url.searchParams.delete("hud");
    history.replaceState(history.state, "", `${url.pathname}${url.search}${url.hash}`);
  });
}
let previewOpenDrive = false;
let previewGrounded = false;
let localLapTarget = 2;
// Cars a solo drive on an installed track draws its field with (set by the scene source once their models are prepared).
let soloRoster = null;
let projectTrackCatalog = [];
let activeProjectTrack = null;
/** Scenic project track shown in the lobby preview and preselected for "Drive solo". */
const MENU_PREVIEW_TRACK = "pine-ridge";
const hz = 60;
const stepSeconds = 1 / hz;

function applyModePresentation() {
  const params = new URLSearchParams(location.search);
  const localPractice = params.get("practice") === "local";
  const privateMode = privateSceneRequested ? sceneSourceHook.privateMode : null;
  document.querySelector("#mode-label").textContent = privateMode
    ? (localPractice ? privateMode.localLabel : privateMode.networkLabel)
    : localPractice ? "Solo Practice · Test Circuit" : "Race Lobby · Test Circuit";
  document.querySelector("#practice-entry").hidden = localPractice;
  document.querySelector("#practice-session").hidden = !localPractice;
  document.querySelector(".network-panel").hidden = localPractice;
  if (localPractice && privateMode) {
    document.querySelector("#practice-session h2").textContent = privateMode.title;
    document.querySelector("#practice-session p").textContent = privateMode.description;
  }
  if (localPractice) document.querySelector("#network-status").textContent = "Offline practice · local simulation";
  return localPractice;
}

function setPreviewCameraMode(mode, persistSelection = false) {
  if (!previewCameraNames.has(mode)) return;
  previewCameraMode = mode;
  previewCameraSelect.value = mode;
  if (persistSelection) {
    const url = new URL(location.href);
    url.searchParams.set("camera", mode);
    history.replaceState(history.state, "", `${url.pathname}${url.search}${url.hash}`);
  }
  if (gpu && wasmModule?.set_preview_camera_mode) {
    try {
      wasmModule.set_preview_camera_mode(mode);
    } catch (error) {
      console.warn("Could not select preview camera", error);
    }
  }
}

function setPreviewLookBehind(enabled) {
  if (!gpu || !wasmModule?.set_preview_look_behind) return;
  try {
    wasmModule.set_preview_look_behind(enabled);
    tickEl.dataset.lookBehind = String(enabled);
  } catch (error) {
    console.warn("Could not select preview look-behind", error);
  }
}

function setPreviewMirrorVisible(visible) {
  previewMirrorVisible = visible;
  tickEl.dataset.mirrorVisible = String(visible);
  if (!gpu) return;
  try {
    wasmModule.set_preview_mirror_visible(visible);
  } catch (error) {
    console.warn("Could not select preview mirror visibility", error);
  }
}

function cyclePreviewCamera() {
  const modes = ["chase", "wide", "close", "road"];
  const current = modes.indexOf(previewCameraMode);
  setPreviewCameraMode(modes[(current + 1) % modes.length], true);
}

function releasePreviewLookBehind() {
  keys.delete("KeyB");
  setPreviewLookBehind(false);
}

/** Gearbox mode of the local simulation (solo driving and room prediction). */
function applyTransmissionToRace() {
  if (race && typeof race.set_manual_transmission === "function") race.set_manual_transmission(isManual(activeTransmission()));
  if (race && typeof race.set_auto_reverse === "function") race.set_auto_reverse(autoReverseEnabled);
}

function isEditableTarget(target) {
  return target instanceof Element
    && (target.isContentEditable || target.closest("input, textarea, select, [contenteditable='true']"));
}

const localPracticeMode = applyModePresentation();
const soloOpponents = localPracticeMode && !privateSceneRequested
  ? parseOpponentParams(location.search)
  : { count: 0, skill: 1, catchUp: true, laps: 1 };
let localOpponentStates = [];
const previewCameraSelect = document.querySelector("#preview-camera-mode");
const speedDisplaySelect = document.querySelector("#speed-display-unit");
let speedDisplayUnit = readSpeedUnit(location.search);
speedDisplaySelect.value = speedDisplayUnit;
speedDisplaySelect.addEventListener("change", () => {
  speedDisplayUnit = ["mph", "raw"].includes(speedDisplaySelect.value) ? speedDisplaySelect.value : "kmh";
  history.replaceState(history.state, "", speedUnitPreferenceLocation(location.href, speedDisplayUnit));
});
const soundPreference = readSoundPreference(location.search);
const gameAudio = new GameAudio(soundPreference);
const impactDetector = new ImpactDetector();
let lastAudioMs = null;
const soundMuteInput = document.querySelector("#sound-muted");
const soundVolumeInput = document.querySelector("#sound-volume");
soundMuteInput.checked = soundPreference.muted;
soundVolumeInput.value = String(soundPreference.volume);
function onSoundPreferenceChange() {
  soundPreference.muted = soundMuteInput.checked;
  soundPreference.volume = Number(soundVolumeInput.value);
  gameAudio.setMuted(soundPreference.muted);
  gameAudio.setVolume(soundPreference.volume);
  history.replaceState(history.state, "", soundPreferenceLocation(location.href, soundPreference));
}
soundMuteInput.addEventListener("change", onSoundPreferenceChange);
soundVolumeInput.addEventListener("input", onSoundPreferenceChange);
// Browsers require a user gesture before audio may start.
for (const type of ["keydown", "pointerdown"]) window.addEventListener(type, () => gameAudio.unlock());
window.addEventListener("pagehide", () => gameAudio.dispose());
const previewCameraNames = new Set(["chase", "wide", "close", "road"]);
const requestedPreviewCamera = new URLSearchParams(location.search).get("camera");
let previewCameraMode = previewCameraNames.has(requestedPreviewCamera)
  ? requestedPreviewCamera
  : requestedPreviewCamera === "hood" ? "road" : "chase";
previewCameraSelect.value = previewCameraMode;
previewCameraSelect.addEventListener("change", () => setPreviewCameraMode(previewCameraSelect.value, true));
const frameRecoveryNotice = document.createElement("p");
frameRecoveryNotice.id = "frame-recovery-notice";
frameRecoveryNotice.className = "scene-status";
frameRecoveryNotice.setAttribute("role", "alert");
frameRecoveryNotice.hidden = true;
const frameRecoveryButton = document.createElement("button");
frameRecoveryButton.type = "button";
frameRecoveryButton.id = "frame-recovery-button";
frameRecoveryButton.className = "secondary";
frameRecoveryButton.textContent = "Reload game";
frameRecoveryButton.hidden = true;
(document.querySelector("#frame-recovery-slot") ?? document.body).append(frameRecoveryNotice, frameRecoveryButton);
frameRecoveryButton.addEventListener("click", () => window.location.reload());
const networkTrackChoice = document.querySelector("#network-track-choice");
let networkTrackPickedByUser = false;
networkTrackChoice?.addEventListener("change", () => { networkTrackPickedByUser = true; });
const networkDriverName = document.querySelector("#network-driver-name");
const networkPaintChoice = document.querySelector("#network-paint-choice");
const roomTrackChoice = document.querySelector("#room-track-choice");
const roomVehicleChoice = document.querySelector("#room-vehicle-choice");
const roomLapsChoice = document.querySelector("#room-laps");
const roomPaintChoice = document.querySelector("#room-paint-choice");


function currentPaintCount() {
  return paintChoiceCount(vehicleData);
}

function effectivePaintColour() {
  return clampPaintColour(paintColour ?? scenePaintColour ?? 0, currentPaintCount());
}

function populatePaintSelect(select, colour) {
  const count = currentPaintCount();
  reconcileSelectOptions(
    select,
    paintChoiceOptions(count),
    paintIdForColour(clampPaintColour(colour, count)),
    select.disabled,
    ({ value, label }) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      return option;
    },
  );
}

function roomSummary() {
  return latestLobby.find((item) => item.race_id === activeRaceId) ?? null;
}

function captureRoomWorkToken() {
  const summary = roomSummary();
  return {
    raceId: activeRaceId,
    selectionRevision: summary?.selection_revision,
    epoch: roomTemplateSyncEpoch,
  };
}

function isCurrentRoomWorkToken(token) {
  return isCurrentRoomWork(token, currentRoomWorkState());
}

function currentRoomWorkState() {
  const summary = roomSummary();
  const revision = authoritative?.selection_revision === summary?.selection_revision
    ? summary?.selection_revision : null;
  return {
    raceId: activeRaceId,
    selectionRevision: revision,
    epoch: roomTemplateSyncEpoch,
  };
}

// Scene sources (docs/PLUGINS.md, "sceneSource"): a plugin may import scenes from local files.
// It receives this context object; live page state is exposed through accessors.
const sceneSourceHost = Object.defineProperties({
  statusEl, lapEl, tickEl, localPracticeMode, gameAudio, networkPaintChoice,
  hud: canvasHud,
  paint: { effective: effectivePaintColour, populate: populatePaintSelect },
  roomSummary, captureRoomWorkToken, isCurrentRoomWorkToken, currentRoomWorkState, ifCurrentRoomWork,
  applyRouteMap, sendVehicleModelMessage, updateNetworkTrackChoice,
  setSpeedUnit(unit) { speedDisplayUnit = unit; speedDisplaySelect.value = unit; },
}, Object.fromEntries([
  ["race", () => race], ["soloOpponents", () => soloOpponents], ["wasm", () => wasmModule], ["gpu", () => gpu], ["playerId", () => playerId],
  ["activeRaceId", () => activeRaceId], ["connected", () => connected],
  ["networkSessionRevision", () => networkSessionRevision], ["authoritative", () => authoritative],
  ["availableTracks", () => latestAvailableTracks],
].map(([name, get]) => [name, { get, enumerable: true }])
  .concat([
    ["simulationSpawnPose", () => simulationSpawnPose, (v) => { simulationSpawnPose = v; }],
    ["roomSelectionRevision", () => roomSelectionRevision, (v) => { roomSelectionRevision = v; }],
    ["roomTemplateSyncSignature", () => roomTemplateSyncSignature, (v) => { roomTemplateSyncSignature = v; }],
    ["roomTemplateSyncPromise", () => roomTemplateSyncPromise, (v) => { roomTemplateSyncPromise = v; }],
    ["roomTemplateSyncError", () => roomTemplateSyncError, (v) => { roomTemplateSyncError = v; }],
    ["roomTemplateFailedSignature", () => roomTemplateFailedSignature, (v) => { roomTemplateFailedSignature = v; }],
    ["roomTemplateSyncEpoch", () => roomTemplateSyncEpoch, (v) => { roomTemplateSyncEpoch = v; }],
    ["roomTemplateReadySignature", () => roomTemplateReadySignature, (v) => { roomTemplateReadySignature = v; }],
    ["registeredVehicleProfiles", () => registeredVehicleProfiles, (v) => { registeredVehicleProfiles = v; }],
    ["registeredTrackFingerprint", () => registeredTrackFingerprint, (v) => { registeredTrackFingerprint = v; }],
    ["activeSceneTrackFingerprint", () => activeSceneTrackFingerprint, (v) => { activeSceneTrackFingerprint = v; }],
    ["currentRoomTemplateProfiles", () => currentRoomTemplateProfiles, (v) => { currentRoomTemplateProfiles = v; }],
    ["privateScenePose", () => privateScenePose, (v) => { privateScenePose = v; }],
    ["privateSceneRevision", () => privateSceneRevision, (v) => { privateSceneRevision = v; }],
    ["previewOpenDrive", () => previewOpenDrive, (v) => { previewOpenDrive = v; }],
    ["localLapTarget", () => localLapTarget, (v) => { localLapTarget = v; }],
    ["soloRoster", () => soloRoster, (v) => { soloRoster = v; }],
    ["previewGrounded", () => previewGrounded, (v) => { previewGrounded = v; }],
    ["paintColour", () => paintColour, (v) => { paintColour = v; }],
    ["scenePaintColour", () => scenePaintColour, (v) => { scenePaintColour = v; }],
    ["vehicleData", () => vehicleData, (v) => { vehicleData = v; }],
  ].map(([name, get, set]) => [name, { get, set, enumerable: true }]))));
const sceneSource = RacerPlugins.invoke("sceneSource", "mount", [sceneSourceHost], null);
const synchronizeRoomVehicleTemplates = () => (sceneSource ? sceneSource.syncRoomTemplates() : Promise.resolve([]));
const vehicleStem = (profileId) => sceneSource?.vehicleStem?.(profileId) ?? null;


try {
  wasmModule = await import("./pkg/racer_client.js");
  requireMirrorVisibilityApi(wasmModule);
  await wasmModule.default();
  race = new wasmModule.BrowserRace(hz);
  applyTransmissionToRace();
  if (soloOpponents.count > 0 && typeof race.configure_opponents === "function") {
    race.configure_opponents(soloOpponents.count, soloOpponents.skill, soloOpponents.catchUp);
  }
  simulationSpawnPose = { x: race.x(), y: race.y(), heading: race.heading() };
  connectLobby();
  resize();
  try {
    await wasmModule.init_wgpu(gpuCanvas.id);
    gpu = wasmModule.gpu_ready();
    if (!gpu) throw new Error("WebGPU renderer did not become ready");
  } catch (error) {
    console.warn("WebGPU renderer initialization failed; trying wgpu WebGL2", error);
    try {
      await wasmModule.init_webgl(webglCanvas.id);
      gpu = wasmModule.gpu_ready();
      if (!gpu) throw new Error("WebGL2 renderer did not become ready");
      canvas = webglCanvas;
    } catch (webglError) {
      console.warn("wgpu WebGL2 initialization failed; using Canvas 2D fallback", webglError);
    }
  }
  statusEl.textContent = gpu ? `WASM + wgpu ${wasmModule.gpu_backend()} renderer · 60 Hz` : "3D rendering unavailable · 60 Hz";
} catch (error) {
  // This lets the source tree run from a static host before the optional WASM
  // package has been built; the installed Rust facade is used when available.
  statusEl.textContent = error?.message?.startsWith("Generated browser WASM bindings are stale")
    ? error.message
    : "Local simulation · 60 Hz";
  if (!localPracticeMode) document.querySelector("#network-status").textContent = "Race lobby needs the WASM network codec. Solo practice is available above.";
}
if (gpu) {
  fallbackCanvas.hidden = true;
  gpuCanvas.hidden = canvas !== gpuCanvas;
  webglCanvas.hidden = canvas !== webglCanvas;
  previewCameraSelect.disabled = false;
  setPreviewCameraMode(previewCameraMode);
  setPreviewLookBehind(false);
  setPreviewMirrorVisible(previewMirrorVisible);
} else {
  previewCameraSelect.disabled = true;
  useCanvasFallback();
}

// Project-owned demo cars (no imported data needed): the player's pick plus a
// field of different opponent cars on the project circuit.
const projectCarSetup = document.querySelector("#project-car-setup");
const projectCarChoice = document.querySelector("#project-car-choice");
const projectPaintChoice = document.querySelector("#project-paint-choice");
const projectCarSwatch = document.querySelector("#project-car-swatch");
let projectCarCatalog = null;
let projectCarSelection = null;

function fillProjectPaintOptions() {
  projectPaintChoice.replaceChildren(...paintOptions(projectCarCatalog, projectCarSelection.car).map((spec) => {
    const option = document.createElement("option");
    option.value = spec.value;
    option.textContent = spec.label;
    return option;
  }));
  projectPaintChoice.value = projectCarSelection.paint;
  const paint = paintOptions(projectCarCatalog, projectCarSelection.car).find((spec) => spec.value === projectCarSelection.paint);
  projectCarSwatch.style.background = paint?.colour ?? "#808080";
}

function installProjectCars() {
  try {
    wasmModule.install_project_scene(projectCarSelection.car, projectCarSelection.paint, fieldOpponents(soloOpponents.count, !localPracticeMode));
    tickEl.dataset.projectCar = describeSelection(projectCarSelection);
    gameAudio.setEngineSound(projectCarCatalog?.cars.find((car) => car.id === projectCarSelection.car)?.engineSound ?? null);
    tickEl.dataset.projectField = typeof wasmModule.project_field_ids === "function" ? wasmModule.project_field_ids() : "";
    return true;
  } catch (error) {
    console.warn("Could not install the project demo cars", error);
    tickEl.dataset.projectCar = "";
    return false;
  }
}

/** The chosen demo car for scene installs (empty ids select the first car). */
function projectCarArgs() {
  return projectCarSelection ?? { car: "", paint: "" };
}

/** Cars of the solo field as flat index pairs, the driver first (cached per selection). */
let projectRosterCache = { key: "", bytes: null };
function projectRoster() {
  if (!projectCarCatalog || typeof wasmModule?.project_roster !== "function") return null;
  const indices = selectionIndices(projectCarCatalog, projectCarSelection);
  if (!indices) return null;
  const key = indices.join(",");
  if (projectRosterCache.key !== key) projectRosterCache = { key, bytes: wasmModule.project_roster(indices[0], indices[1], 7) };
  return projectRosterCache.bytes;
}

/** Packed car picks for the vehicles of a frame, or an empty list when the demo cars do not apply. */
function packedCarPicks(vehicles) {
  if (!projectCarCatalog || !activeProjectTrack || vehicles.length === 0) return new Uint8Array(0);
  return packPicks(vehicles.map((vehicle) => vehicle.carPick ?? null));
}

/** Car pick of a server vehicle snapshot (its own choice or the server's assignment). */
function snapshotCarPick(vehicle) {
  return projectCarCatalog ? wireIndices(projectCarCatalog, vehicle?.car_model, vehicle?.car_variant) : null;
}

let renderedCarsPublishedAt = 0;
/** Publish the cars drawn by the last frame for diagnostics and tests (a few times a second). */
function publishRenderedCars() {
  const now = performance.now();
  if (now - renderedCarsPublishedAt < 250 || typeof wasmModule?.project_render_picks !== "function") return;
  renderedCarsPublishedAt = now;
  tickEl.dataset.projectRender = wasmModule.project_render_picks();
}

function reinstallProjectTrackCars() {
  if (!activeProjectTrack || typeof wasmModule?.reinstall_project_track_cars !== "function") return;
  try {
    const car = projectCarArgs();
    wasmModule.reinstall_project_track_cars(car.car, car.paint, activeProjectTrack.room ? 0 : fieldOpponents(soloOpponents.count, false));
    tickEl.dataset.projectCar = describeSelection(car);
    tickEl.dataset.projectField = typeof wasmModule.project_field_ids === "function" ? wasmModule.project_field_ids() : "";
  } catch (error) {
    console.warn("Could not install the demo cars on the generated track", error);
  }
}

/** Tell the room which car this player picked (lobby only; the server revalidates it). */
function announceProjectCar() {
  if (!connected || activeRaceId === null || !projectCarCatalog) return;
  const summary = roomSummary();
  const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  if (!summary || summary.open_drive || !own || statusLabel(summary.status) !== "Lobby") return;
  sendRoomSelectionChange({ change_vehicle: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    vehicle_profile_id: own.vehicle_profile_id,
    vehicle_fingerprint: own.vehicle_fingerprint,
    paint_id: own.paint_id,
    ...wireFields(projectCarCatalog, projectCarSelection),
  } });
}

function setupProjectCars() {
  if (typeof wasmModule?.project_car_catalog !== "function" || typeof wasmModule?.install_project_scene !== "function") return;
  try {
    projectCarCatalog = parseProjectCatalog(wasmModule.project_car_catalog());
    let stored = null;
    try { stored = readStoredProjectCar(window.localStorage); } catch { /* storage may be blocked */ }
    projectCarSelection = selectionFromSources(projectCarCatalog, location.search, stored);
    projectCarChoice.replaceChildren(...carOptions(projectCarCatalog).map((spec) => {
      const option = document.createElement("option");
      option.value = spec.value;
      option.textContent = spec.label;
      return option;
    }));
    projectCarChoice.value = projectCarSelection.car;
    fillProjectPaintOptions();
    tickEl.dataset.projectCarCount = String(projectCarCatalog.cars.length);
    if (!installProjectCars()) {
      projectCarCatalog = null;
      return;
    }
    const changed = () => {
      projectCarSelection = resolveProjectSelection(projectCarCatalog, projectCarChoice.value, projectPaintChoice.value);
      projectCarChoice.value = projectCarSelection.car;
      fillProjectPaintOptions();
      try { writeStoredProjectCar(window.localStorage, projectCarSelection); } catch { /* storage may be blocked */ }
      if (!privateScenePose) installProjectCars();
      else if (activeProjectTrack) reinstallProjectTrackCars();
      announceProjectCar();
    };
    projectCarChoice.addEventListener("change", () => {
      // A new model starts on its first paint unless the current paint exists for it.
      projectCarSelection = { car: projectCarChoice.value, paint: projectPaintChoice.value };
      changed();
    });
    projectPaintChoice.addEventListener("change", changed);
  } catch (error) {
    console.warn("Project demo cars are unavailable", error);
    projectCarCatalog = null;
  }
}

function syncProjectCarVisibility() {
  const visible = projectCarCatalog !== null && (!privateScenePose || activeProjectTrack !== null);
  if (projectCarSetup.hidden === visible) projectCarSetup.hidden = !visible;
}

if (gpu && !privateSceneRequested) {
  setupProjectCars();
  await sceneSource?.restoreSaved?.();
}

if (gpu && privateSceneRequested) await sceneSource?.loadProviderScene?.();

if (gpu && !privateSceneRequested) void sceneSource?.revealProviderEntry?.();

// Generated project tracks (racer-trackgen): catalog menus and solo activation.
if (typeof wasmModule?.project_tracks_json === "function") {
  try { projectTrackCatalog = parseCatalog(wasmModule.project_tracks_json()); } catch (error) { console.warn("Could not read the project track catalog", error); }
}
populateProjectTrackMenus();
if (gpu && race && localPracticeMode && !privateSceneRequested) {
  const requestedTrack = requestedSoloTrack();
  if (requestedTrack) activateProjectTrack(requestedTrack);
} else if (gpu && race && !localPracticeMode && !privateSceneRequested && menuPreviewTrackAvailable()) {
  // The lobby preview shows the scenic default track instead of the bare test circuit.
  activateProjectTrack(MENU_PREVIEW_TRACK, { menu: true });
}

// Multiplayer rooms on generated tracks: the renderer shows the room's track and
// server vehicles are mapped through the same simulation-to-scene pose.
function activateProjectRoomScene(trackId) {
  const entry = resolveCatalogEntry(projectTrackCatalog, trackId);
  if (!entry || typeof wasmModule?.install_project_track !== "function") return false;
  try {
    const car = projectCarArgs();
    const metadata = JSON.parse(wasmModule.install_project_track(trackId, car.car, car.paint, 0));
    privateScenePose = projectTrackPose(metadata, { x: 0, y: 0, heading: 0 });
    privateSceneRevision++;
    previewGrounded = false;
    activeProjectTrack = { id: entry.id, name: entry.name, fingerprint: entry.fingerprint, laps: metadata.laps, room: true };
    hudTrackMap = buildTrackMap(metadata.map); hudWrongWay?.reset();
    registeredVehicleProfiles.clear();
    registeredTrackFingerprint = null;
    activeSceneTrackFingerprint = null;
    applyRouteMap(null);
    document.querySelector("#mode-label").textContent = `Race room · ${entry.name}`;
    tickEl.dataset.projectTrack = entry.id;
    tickEl.dataset.projectCar = describeSelection(car);
    tickEl.dataset.projectField = typeof wasmModule.project_field_ids === "function" ? wasmModule.project_field_ids() : "";
    return true;
  } catch (error) {
    console.warn("Could not install the room's generated track", error);
    document.querySelector("#network-status").textContent = `Could not load the room's track: ${error?.message ?? error}`;
    return false;
  }
}

function currentProjectRoomTrackId() {
  const summary = roomSummary();
  return activeRaceId !== null && summary ? projectTrackIdFromProfile(summary.track_profile_id) : null;
}

// Keep the renderer scene in step with the room the player is in.
function syncProjectRoomScene() {
  if (!gpu || localPracticeMode || !connected) return;
  const trackId = currentProjectRoomTrackId();
  if (trackId) {
    if (activeProjectTrack?.room !== true || activeProjectTrack.id !== trackId) activateProjectRoomScene(trackId);
  } else if (activeProjectTrack?.room === true || (activeProjectTrack?.menu === true && activeRaceId !== null)) {
    activeProjectTrack = null;
    delete tickEl.dataset.projectTrack;
    menuPreviewRestoreTried = false;
    void restoreProjectSceneForGenericRoom().then(restoreMenuPreview);
  } else if (activeRaceId !== null) {
    menuPreviewRestoreTried = false;
  } else {
    restoreMenuPreview();
  }
}

let menuPreviewRestoreTried = false;
/** Back in the lobby (after leaving any room) the scenic preview returns instead of the bare test circuit. */
function restoreMenuPreview() {
  if (menuPreviewRestoreTried || activeRaceId !== null || localPracticeMode || privateSceneRequested
      || activeProjectTrack !== null || !menuPreviewTrackAvailable()) return;
  menuPreviewRestoreTried = true;
  activateProjectTrack(MENU_PREVIEW_TRACK, { menu: true });
}

/**
 * The scenic preview is on by default. Automated browsers (software GL) keep the
 * light test circuit unless `?menupreview=1` asks for it; `?menupreview=0` turns it off.
 */
function menuPreviewWanted() {
  const requested = new URLSearchParams(location.search).get("menupreview");
  if (requested === "1") return true;
  if (requested === "0") return false;
  return !navigator.webdriver;
}

function scenicTrackAvailable() {
  return menuPreviewWanted()
    && projectTrackCatalog.some((track) => track.id === MENU_PREVIEW_TRACK)
    && typeof wasmModule?.install_project_track === "function";
}

function menuPreviewTrackAvailable() {
  return !parseTrackParam(location.search) && scenicTrackAvailable();
}

/**
 * Track of a solo drive: the `track` parameter; without one the scenic default (Pine Ridge);
 * `track=test-circuit` is the explicit choice of the bare circuit. Null = test circuit.
 */
function requestedSoloTrack() {
  const requested = parseTrackParam(location.search);
  if (requested === TEST_CIRCUIT_TRACK_ID) return null;
  return requested ?? (scenicTrackAvailable() ? MENU_PREVIEW_TRACK : null);
}

function trackThemeLabel(theme) {
  const labels = { "pine-ridge": "alpine forest", coastline: "coastal road", "red-mesa": "desert canyon", "night-harbour": "night harbour" };
  return labels[theme] ?? String(theme).replace("-", " ");
}

/** Title and blurb of the solo entry card follow the selected track. */
function syncSoloEntryCopy() {
  const title = document.querySelector("#solo-entry-title");
  const blurb = document.querySelector("#solo-entry-blurb");
  const select = document.querySelector("#solo-track-choice");
  if (!title || !blurb || !select) return;
  const track = projectTrackCatalog.find((candidate) => candidate.id === select.value);
  title.textContent = track ? `Practice on ${track.name}` : "Practice on the Test Circuit";
  blurb.textContent = track
    ? `${track.description} ${track.laps} laps, ${trackThemeLabel(track.theme)} scenery generated by the project.`
    : "Drive the project course with a local simulation. No server or game data needed.";
}

function populateProjectTrackMenus() {
  const entry = document.querySelector("#solo-track-choice");
  const entryWrap = document.querySelector("#solo-track-choice-wrap");
  const switcher = document.querySelector("#solo-track-switch");
  const switcherWrap = document.querySelector("#solo-track-switch-wrap");
  if (!entry || !switcher || projectTrackCatalog.length === 0) return;
  for (const select of [entry, switcher]) {
    for (const track of projectTrackCatalog) {
      const option = document.createElement("option");
      option.value = track.id;
      option.textContent = `${track.name} · ${trackThemeLabel(track.theme)}`;
      option.title = track.description;
      select.append(option);
    }
  }
  entryWrap.hidden = false;
  switcherWrap.hidden = false;
  // With the scenic default on, the circuit is an explicit choice (`track=test-circuit`), so it can be picked again.
  if (scenicTrackAvailable()) for (const select of [entry, switcher]) select.options[0].value = TEST_CIRCUIT_TRACK_ID;
  const selected = requestedSoloTrack() ?? (scenicTrackAvailable() ? TEST_CIRCUIT_TRACK_ID : "");
  entry.value = selected;
  switcher.value = selected;
  entry.addEventListener("change", syncSoloEntryLink);
  entry.addEventListener("change", syncSoloEntryCopy);
  syncSoloEntryCopy();
  switcher.addEventListener("change", () => { location.search = soloQuery(currentSoloSettings("practice")); });
}

function activateProjectTrack(trackId, { menu = false } = {}) {
  const entry = resolveCatalogEntry(projectTrackCatalog, trackId);
  if (!entry || typeof wasmModule?.install_project_track !== "function") {
    statusEl.textContent = "Unknown track · showing the test circuit";
    return;
  }
  try {
    const started = performance.now();
    const car = projectCarArgs();
    const metadata = JSON.parse(wasmModule.install_project_track(trackId, car.car, car.paint, menu ? 0 : fieldOpponents(soloOpponents.count, false)));
    race.use_project_track();
    if (!menu && soloOpponents.count > 0 && typeof race.configure_opponents === "function") {
      race.configure_opponents(soloOpponents.count, soloOpponents.skill, soloOpponents.catchUp);
    }
    simulationSpawnPose = { x: race.x(), y: race.y(), heading: race.heading() };
    previewGrounded = race.preview_ground_enabled();
    // the simulation frame is anchored at grid slot 0 with heading 0
    privateScenePose = projectTrackPose(metadata, { x: 0, y: 0, heading: 0 });
    privateSceneRevision++;
    previewOpenDrive = false;
    localLapTarget = metadata.laps;
    activeProjectTrack = { id: entry.id, name: entry.name, fingerprint: entry.fingerprint, laps: metadata.laps, room: false, menu };
    hudTrackMap = buildTrackMap(metadata.map); hudWrongWay?.reset();
    applyRouteMap(null);
    if (!menu) statusEl.textContent = `${entry.name} · wgpu ${wasmModule.gpu_backend()} · 60 Hz`;
    if (menu) {
      document.querySelector("#mode-label").textContent = `Race Lobby · ${entry.name}`;
    } else {
      document.querySelector("#mode-label").textContent = `Solo Practice · ${entry.name}`;
      document.querySelector("#practice-session h2").textContent = entry.name;
      document.querySelector("#practice-session p").textContent = `${entry.description} Generated by the project track generator; ${metadata.laps} laps.`;
    }
    tickEl.dataset.projectTrack = entry.id;
    tickEl.dataset.projectCar = describeSelection(car);
    tickEl.dataset.projectField = typeof wasmModule.project_field_ids === "function" ? wasmModule.project_field_ids() : "";
    tickEl.dataset.projectTrackLoadMs = String(Math.round(performance.now() - started));
  } catch (error) {
    console.warn("Could not activate the generated project track", error);
    statusEl.textContent = `Track unavailable: ${error?.message ?? error}`;
  }
}



function useCanvasFallback() {
  gpuCanvas.hidden = true;
  webglCanvas.hidden = true;
  fallbackCanvas.hidden = false;
  canvas = fallbackCanvas;
  ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) statusEl.textContent = "Canvas rendering unavailable";
}

// Gear requests are edge events; the latch keeps a press that is shorter than the
// frame/network sampling interval until it has been handed to the simulation.
const shiftLatch = createShiftLatch();
const gearHints = createGearHintTracker();
if (autoReverseInput) {
  autoReverseInput.checked = autoReverseEnabled;
  autoReverseInput.addEventListener("change", () => {
    autoReverseEnabled = autoReverseInput.checked;
    try { window.localStorage.setItem("open-web-racer.autoReverse", autoReverseEnabled ? "1" : "0"); } catch { /* storage may be blocked */ }
    sendMessage(setTransmissionMessage(networkTransmission, autoReverseEnabled));
    applyTransmissionToRace();
    refreshControlsHint();
  });
}

function controlState() {
  const gamepad = navigator.getGamepads?.().find((pad) => pad?.connected) ?? null;
  const keyboardThrottle = keys.has("ArrowUp") || keys.has("KeyW");
  const keyboardBrake = keys.has("ArrowDown") || keys.has("KeyS");
  const axis = gamepad?.axes[0] ?? 0;
  return {
    throttle: keyboardThrottle ? 1 : Math.max(0, gamepad?.buttons[7]?.value ?? 0),
    brake: keyboardBrake ? 1 : Math.max(0, gamepad?.buttons[6]?.value ?? 0),
    // Heading grows counterclockwise in the XY simulation plane, so left is positive.
    // A positive gamepad X axis means stick-right and must map to negative steer.
    steer: (keys.has("ArrowLeft") ? 1 : 0) + (keys.has("ArrowRight") ? -1 : 0) || -axis,
    handbrake: keys.has("Space") || !!gamepad?.buttons[0]?.pressed,
    // Classic bindings: A shifts up, Z shifts down (matched by physical key, not layout).
    // Gamepad: right bumper shifts up, left bumper shifts down.
    shift_up: keys.has("KeyA") || !!gamepad?.buttons[5]?.pressed,
    shift_down: keys.has("KeyZ") || !!gamepad?.buttons[4]?.pressed,
  };
}

/** Own gear code and speed for the hint tracker (authoritative in rooms, local sim in solo). */
function ownGearSample() {
  if (connected) {
    const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
    return { live: raceStatusKind(authoritative?.status) === "racing" && !!own, gear: own?.gear, speed: own?.speed };
  }
  return race ? { live: true, gear: race.gear(), speed: race.speed() } : { live: false };
}

function updateGearHint(input) {
  const sample = ownGearSample();
  const result = gearHints.update({
    dt: stepSeconds, live: sample.live, gear: sample.gear, speed: sample.speed,
    throttle: input.throttle, brake: input.brake, autoReverse: autoReverseEnabled,
  });
  if (gearHintEl) {
    const text = result.hint ?? "";
    if (gearHintEl.textContent !== text) gearHintEl.textContent = text;
    gearHintEl.hidden = !text;
  }
}

function fixedStep() {
  const input = controlState();
  shiftLatch.observe({ up: input.shift_up, down: input.shift_down });
  updateGearHint(input);
  Object.assign(input, shiftLatch.merged({ up: input.shift_up, down: input.shift_down }));
  if (connected) {
    if (authoritative?.status?.racing !== undefined || authoritative?.status === "racing") {
      networkElapsed++;
      if (networkElapsed >= 3) { networkElapsed = 0; sendDriverInput(input, keys.has("KeyR")); shiftLatch.consume(); }
    } else { networkElapsed = 0; shiftLatch.clear(); }
    return;
  }
  if (race) {
    race.set_controls(input.throttle, input.brake, input.steer, input.handbrake);
    if (typeof race.set_shift === "function") race.set_shift(input.shift_up, input.shift_down);
    race.tick();
    shiftLatch.consume();
  } else {
    const speed = Math.hypot(fallback.vx, fallback.vy);
    fallback.vx += Math.cos(fallback.heading) * (input.throttle * 12 - input.brake * 20) * stepSeconds;
    fallback.vy += Math.sin(fallback.heading) * (input.throttle * 12 - input.brake * 20) * stepSeconds;
    const drag = 1 - 0.12 * stepSeconds;
    fallback.vx *= drag; fallback.vy *= drag;
    const nextSpeed = Math.hypot(fallback.vx, fallback.vy);
    if (nextSpeed > 45) { fallback.vx *= 45 / nextSpeed; fallback.vy *= 45 / nextSpeed; }
    const steerGain = Math.min(1, Math.max(.15, speed / 8)) * (input.handbrake ? 1.5 : 1);
    fallback.heading = (fallback.heading + input.steer * 1.8 * steerGain * stepSeconds) % (Math.PI * 2);
    const previousPosition = { x: fallback.x, y: fallback.y };
    fallback.x += fallback.vx * stepSeconds; fallback.y += fallback.vy * stepSeconds;
    const gates = [[-20, 0], [0, 12], [20, 0], [0, -12]];
    const gate = gates[fallback.checkpoint];
    if (distanceToSegment(gate[0], gate[1], previousPosition.x, previousPosition.y, fallback.x, fallback.y) <= 5) {
      if (fallback.checkpoint === 0) fallback.lap++;
      fallback.checkpoint = (fallback.checkpoint + 1) % gates.length;
    }
    if (fallback.x < -30 || fallback.x > 30 || fallback.y < -20 || fallback.y > 20) {
      fallback.x = Math.max(-30, Math.min(30, fallback.x)); fallback.y = Math.max(-20, Math.min(20, fallback.y));
      fallback.vx = 0; fallback.vy = 0;
    }
    fallback.tick++;
  }
}

function distanceToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay, lengthSquared = dx * dx + dy * dy;
  if (!lengthSquared) return Math.hypot(px - ax, py - ay);
  const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / lengthSquared));
  return Math.hypot(px - (ax + dx * t), py - (ay + dy * t));
}

function snapshot() {
  if (connected) {
    const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
    if (own) return { ...own, lap: own.completed_laps + 1, tick: authoritative.server_tick, rpm01: (own.rpm_q8 ?? 0) / 255 };
    return { x: -20, y: 0, heading: 0, speed: 0, lap: 1, checkpoint: 0, tick: authoritative?.server_tick ?? 0 };
  }
  return race ? { x: race.x(), y: race.y(), z: race.z(), vx: race.velocity_x(), vy: race.velocity_y(), heading: race.heading(), groundNormal: [race.ground_normal_x(), race.ground_normal_y(), race.ground_normal_z()], speed: race.speed(), gear: race.gear(), rpm01: race.rpm01(), handling_model: typeof race.handling_model_id === "function" ? race.handling_model_id() : undefined, lap: race.lap(), tick: Number(race.tick_count()), race_position: race.race_position(), competitors: typeof race.competitors === "function" ? race.competitors() : 1, current_lap_ticks: race.current_lap_ticks(), last_lap_ticks: race.last_lap_ticks() ?? null, best_lap_ticks: race.best_lap_ticks() ?? null, finish_ticks: race.finish_ticks() ?? null }
    : { ...fallback, speed: Math.hypot(fallback.vx, fallback.vy) };
}

function privateRenderPose(vehicle) {
  const mapped = privateRenderPositionHeading(vehicle);
  const normal = vehicle.groundNormal ?? [0, 0, 1];
  const sceneNormal = wasmModule.transform_preview_normal_to_scene(
    normal[0], normal[1], normal[2], privateScenePose.headingOffset,
  );
  return { ...mapped, groundNormal: Array.from(sceneNormal) };
}

function privateRenderPositionHeading(vehicle) {
  const dx = (vehicle.x - privateScenePose.simOrigin[0]) * privateScenePose.unitsPerSimUnit;
  const dy = (vehicle.y - privateScenePose.simOrigin[1]) * privateScenePose.unitsPerSimUnit;
  const cos = Math.cos(privateScenePose.headingOffset);
  const sin = Math.sin(privateScenePose.headingOffset);
  return {
    x: privateScenePose.sceneOrigin[0] + dx * cos - dy * sin,
    y: privateScenePose.sceneOrigin[1] + dx * sin + dy * cos,
    // Grounded preview worlds (local or server) own an absolute scene-space Z:
    // spawn z = support height - vehicle bottom. Do not re-base it on a Z
    // captured before that world existed (stale after switching tracks).
    z: vehicle.absoluteZ === true && Number.isFinite(vehicle.z)
      ? vehicle.z
      : Number.isFinite(vehicle.z)
        ? privateScenePose.sceneOrigin[2] + (vehicle.z - (privateScenePose.simOriginZ ?? 0)) * privateScenePose.unitsPerSimUnit
        : privateScenePose.sceneOrigin[2],
    heading: vehicle.heading + privateScenePose.headingOffset,
  };
}

// The reference path of the active scene feeds the route map and the project wrong-way rule.
function applyRouteMap(descriptor) {
  hudRoutePath = Array.isArray(descriptor?.preview_reference_path_scene) ? descriptor.preview_reference_path_scene : null;
  hudWrongWay?.reset();
  return setRouteMapFromSceneDescriptor(routeMapView, descriptor);
}

function drawRouteMap(snapshot) {
  hudRivals = [];
  if (!privateScenePose) { routeMapView.draw(null); return; }
  const activeVehicle = connected
    ? authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId)
    : race ? snapshot : null;
  if (!activeVehicle || ![activeVehicle.x, activeVehicle.y, activeVehicle.heading].every(Number.isFinite)) {
    routeMapView.draw(null);
    return;
  }
  const pose = privateRenderPositionHeading(activeVehicle);
  routeMapView.draw(pose);
  if (connected && activeRaceId !== null) {
    const ordered = [...(authoritative?.vehicles ?? [])].sort((a, b) => a.player_id - b.player_id);
    ordered.forEach((vehicle, slot) => {
      if (vehicle.player_id === playerId || ![vehicle.x, vehicle.y].every(Number.isFinite)) return;
      const rival = privateRenderPositionHeading(vehicle);
      const spot = routeMapView.locate(rival);
      if (spot) hudRivals.push({ ...spot, slot });
    });
  }
}

// Positions of the own car and the others in the frame of the built-in HUD's mini map.
function computeHudActors(s) {
  const toMap = (vehicle) => (privateScenePose ? privateRenderPositionHeading(vehicle) : vehicle);
  const valid = (vehicle) => [vehicle?.x, vehicle?.y].every(Number.isFinite);
  const player = valid(s) && Number.isFinite(s.heading) ? toMap({ x: s.x, y: s.y, heading: s.heading }) : null;
  const others = connected
    ? (activeRaceId !== null ? [...(authoritative?.vehicles ?? [])].sort((a, b) => a.player_id - b.player_id).filter((vehicle) => vehicle.player_id !== playerId) : [])
    : localOpponentStates;
  const rivalPoses = others.filter(valid).map((vehicle, slot) => ({ ...toMap({ x: vehicle.x, y: vehicle.y, heading: vehicle.heading ?? 0 }), slot }));
  hudActors = { player, rivalPoses };
}

// Finish table rows for the built-in HUD: the room's vehicles by position, or the own car and the computer cars.
function hudResultRows(s, networkState) {
  if (connected) {
    return [...(networkState?.vehicles ?? [])]
      .filter((vehicle) => Number.isInteger(vehicle.race_position) && vehicle.race_position >= 1)
      .sort((a, b) => a.race_position - b.race_position)
      .map((vehicle, index) => ({
        position: vehicle.race_position,
        name: vehicle.player_id === playerId ? "You" : (vehicle.driver_name || `CPU ${index + 1}`),
        ticks: vehicle.finish_ticks ?? null,
        you: vehicle.player_id === playerId,
      }));
  }
  const rows = [{ position: s.race_position, name: "You", ticks: s.finish_ticks ?? null, you: true }];
  localOpponentStates.forEach((car, index) => rows.push({ position: car.position, name: `CPU ${index + 1}`, ticks: null, you: false }));
  return rows.sort((a, b) => a.position - b.position);
}

// Composes the canvas HUD for this frame (`hud` plugin hook) from the values updateHud stored.
function renderCanvasHud(s) {
  const frame = hudContext;
  if (!frame || !hudHook?.buildModel || !hudLapTracker || !hudWrongWay) return;
  const { model, status, networkState, sessionTick } = frame;
  const builtinHud = hudHook.builtin === true;
  const trackMap = builtinHud ? currentHudMap() : null;
  if (builtinHud && trackMap !== hudMapInUse) { hudMapInUse = trackMap; hudWrongWay.reset(); }
  const phase = toHudTicks(sessionTick);
  const live = connected ? status === "racing" || status === "finished" : true;
  const finished = connected ? status === "finished" || model.race?.finished === true : s.finish_ticks != null;
  const ticksOrNull = (value) => (Number.isFinite(value) && value >= 0 ? toHudTicks(value) : null);
  let lap = null;
  if (model.lap) lap = { completed: finished ? model.lap.target : model.lap.current - 1, total: model.lap.target };
  let position = null;
  if (model.race && Number.isInteger(s.race_position) && s.race_position >= 1) {
    position = { rank: s.race_position, entries: connected ? Math.max(1, networkState?.vehicles?.length ?? 1) : Math.max(1, s.competitors ?? 1) };
  }
  const lapState = hudLapTracker.update({
    key: frame.sessionKey,
    phase,
    completedLaps: lap ? lap.completed : null,
    totalLaps: lap ? lap.total : null,
    lastLapTicks: ticksOrNull(s.last_lap_ticks),
    bestLapTicks: ticksOrNull(s.best_lap_ticks),
  });
  const lapTicks = finished && ticksOrNull(s.finish_ticks) !== null ? ticksOrNull(s.finish_ticks) : (ticksOrNull(s.current_lap_ticks) ?? 0);
  let wrongWay = false;
  const routePath = builtinHud ? trackMap?.center : (privateScenePose ? hudRoutePath : null);
  if (routePath && live && Number.isFinite(s.x) && Number.isFinite(s.y) && Number.isFinite(s.heading)) {
    const pose = privateScenePose ? privateRenderPositionHeading(s) : s;
    const forward = Math.cos(s.heading) * (s.vx ?? Math.cos(s.heading)) + Math.sin(s.heading) * (s.vy ?? Math.sin(s.heading));
    const reversing = s.gear === 250 || forward < 0;
    const kmh = convertSpeed(s.speed, "kmh", { metersPerSecond: Number.isInteger(s.gear) && s.gear > 0 });
    wrongWay = hudWrongWay.update(routePath, pose.x, pose.y, pose.heading + (reversing ? Math.PI : 0), kmh ?? 0);
  } else {
    hudWrongWay.reset();
  }
  let rivalName = "";
  if (connected && position && position.rank > 1) {
    const ahead = networkState?.vehicles?.find((vehicle) => vehicle.race_position === position.rank - 1);
    rivalName = String(ahead?.driver_name ?? "").slice(0, 12);
  }
  const speed = Number.parseFloat(model.speed);
  const unitKey = speedDisplayUnit === "kmh" ? "kmh" : speedDisplayUnit;
  const hudModel = hudHook.buildModel({
    live,
    speed: Number.isFinite(speed) ? speed : null,
    speedUnit: unitKey === "raw" ? "u/s" : unitKey === "kmh" ? "km/h" : unitKey,
    gear: Number.isInteger(s.gear) && s.gear > 0 ? s.gear : undefined,
    rpm01: Number.isInteger(s.gear) && s.gear > 0 ? s.rpm01 : undefined,
    timing: model.race ? { scenePhase: phase, lapTicks, liveAt: 0, ...lapState } : null,
    position,
    lap,
    lapTicks,
    phase,
    wrongWay,
    recordTicks: lapState.storedRecord,
    rivalName,
    // used by the built-in HUD only
    transmission: isManual(activeTransmission()) ? "manual" : "automatic",
    countdownTicks: connected && status === "countdown" ? frame.rawStatus?.countdown?.ticks_left ?? null : null,
    sinceStartTicks: frame.sessionTicks,
    finished,
    results: builtinHud && finished ? hudResultRows(s, networkState) : null,
    laps: lapState,
  });
  canvasHud.render(hudModel, { rivals: hudRivals, trackMap, player: hudActors.player, rivalPoses: hudActors.rivalPoses });
  const dataset = hudCanvas.dataset;
  dataset.art = canvasHud.artKind;
  dataset.banner = hudModel.banner;
  dataset.timer = hudModel.timerText?.trim() ?? "";
  dataset.position = hudModel.positionText?.trim() ?? "";
  dataset.lap = hudModel.lapText ?? "";
  dataset.gear = hudModel.gear ?? "";
  dataset.speed = hudModel.speedText ?? "";
  dataset.tach = String(hudModel.tachLevel);
  dataset.record = hudModel.recordText;
  dataset.rivals = String(hudActors.rivalPoses.length);
}

function currentRaceSummary() {
  return activeRaceId === null ? null : latestLobby.find((item) => item.race_id === activeRaceId) ?? null;
}

function raceStatusKind(status) {
  if (typeof status === "string") return status.toLowerCase();
  if (status?.countdown) return "countdown";
  return Object.keys(status ?? {})[0]?.toLowerCase() ?? "";
}

// Race lifecycle events for onRaceEvent plugins; emitted when the session, status or finish flag changes.
let raceEventKey = "";
function emitRaceLifecycle(sessionKey, status, finished, lap) {
  const key = `${sessionKey}|${status}|${finished}`;
  if (key === raceEventKey) return;
  const type = raceEventKey.split("|", 1)[0] !== sessionKey ? "session" : finished ? "finish" : "status";
  raceEventKey = key;
  RacerPlugins.emitRaceEvent({ type, session: sessionKey, status, finished, connected, lap });
}

/** Laps of the race on the installed track in a solo drive (0: free driving or not an installed track). */
function importedRaceLaps() {
  return previewOpenDrive && !connected && typeof race?.imported_laps === "function" ? race.imported_laps() : 0;
}

function updateHud(s) {
  const networkState = connected && authoritative?.race_id === activeRaceId ? authoritative : null;
  const summary = currentRaceSummary();
  const status = connected ? raceStatusKind(networkState?.status ?? summary?.status) : "";
  const sessionKey = connected
    ? `network:${networkSessionRevision}:${activeRaceId ?? "lobby"}`
    : `local:${localPracticeMode ? "practice" : "fallback"}:${privateSceneRevision}:${previewOpenDrive ? "open" : "circuit"}`;
  const sessionTick = connected ? networkState?.server_tick : s.tick;
  const sessionTicks = hudSessionClock.update(sessionKey, sessionTick, {
    running: connected ? status === "racing" : true,
    finished: connected && status === "finished",
  });
  const own = networkState?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  emitRaceLifecycle(sessionKey, status, connected ? status === "finished" : s.finish_ticks != null, s.lap);
  const model = buildHudModel({
    connected,
    localPracticeMode,
    openDrive: previewOpenDrive && !importedRaceLaps(),
    snapshot: s,
    localCompetitors: s.competitors,
    ownVehicle: own,
    networkStatus: networkState?.status,
    inRoom: activeRaceId !== null,
    raceSummary: summary,
    elapsedTicks: sessionTicks,
    localLapTarget: importedRaceLaps() || localLapTarget,
    speedUnit: speedDisplayUnit,
    vehicles: networkState?.vehicles,
  });
  hudTimeLabelEl.textContent = "SESSION";
  hudTimeEl.textContent = model.session;
  hudStateEl.textContent = model.state;
  // Diagnostics: which handling model drives the own car.
  hudStateEl.title = model.handling ?? "";
  tickEl.dataset.handling = model.handling ?? "";
  speedEl.textContent = model.speed;
  speedUnitEl.textContent = model.speedUnitLabel;
  gearSlotEl.hidden = model.gear === undefined;
  if (model.gear !== undefined) gearEl.textContent = model.gear;
  gearSlotEl.dataset.gearKind = gearKind(model.gear === "R" ? 250 : model.gear === "N" ? 251 : Number(model.gear));
  hudRaceEl.hidden = model.race === null;
  if (model.race) {
    hudPositionEl.textContent = model.race.position;
    hudLapTimeLabelEl.textContent = model.race.finished ? "TOTAL" : "LAP TIME";
    hudLapTimeEl.textContent = model.race.lapTime;
    hudLastLapEl.textContent = model.race.lastLap;
    hudBestLapEl.textContent = model.race.bestLap;
  }
  lapCardEl.hidden = model.lap === null;
  hudEl.classList.toggle("has-lap", model.lap !== null);
  if (!lapCardEl.hidden) {
    lapEl.textContent = String(model.lap.current);
    lapTargetEl.textContent = ` / ${model.lap.target}`;
  }
  tickEl.textContent = String(s.tick);
  hudContext = { model, status, networkState, sessionTick: Number.isFinite(sessionTick) ? sessionTick : 0, sessionKey, sessionTicks, rawStatus: networkState?.status ?? summary?.status };
}

function updateAudio(s) {
  const nowMs = performance.now();
  const dt = lastAudioMs === null ? 1 / 60 : Math.min(0.1, Math.max(0.001, (nowMs - lastAudioMs) / 1000));
  lastAudioMs = nowMs;
  if (connected && raceStatusKind(authoritative?.status) !== "racing") { gameAudio.silence(); impactDetector.reset(); return; }
  // rpm01 comes from the calibrated model's gearbox when present (gear > 0);
  // otherwise the audio falls back to its speed-derived estimate.
  const rpm01 = Number.isInteger(s.gear) && s.gear > 0 ? s.rpm01 : undefined;
  const lateral = lateralSpeed(s.vx, s.vy, s.heading);
  gameAudio.setEngineState({ rpm01, throttle: controlState().throttle, speed: s.speed ?? 0 }, dt);
  gameAudio.setSkid(skidLevel(lateral, s.speed ?? 0));
  if (gameAudio.sampledActive) {
    gameAudio.setTyreSlip({ lateralMps: lateral, speed: s.speed ?? 0, racing: true }, dt);
    gameAudio.setHorn(keys.has("KeyH"));
    gameAudio.setRemoteCars(connected ? remoteCarsForAudio(authoritative?.vehicles, playerId, s, vehicleStem) : remoteCarsForAudio(opponentVoices(localOpponentStates), null, s, vehicleStem), dt);
    // Contact events are not exposed to the client yet; a hit shows up as a speed loss faster than braking.
    const impact = impactDetector.update(s.speed ?? 0, dt, lateral);
    if (impact) gameAudio.playImpact(impact);
  }
}

let gpuRenderFailures = 0;
function resize() {
  const rect = canvas.getBoundingClientRect();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.round(rect.width * dpr); canvas.height = Math.round(rect.height * dpr);
}

// Chase-camera yaw follows the body yaw with the observed first-order lag
// (tests/feel); the renderer applies it to the external and look-behind views.
const chaseCameraSmoother = createChaseCameraSmoother();
let chaseCameraLastFrameMs = null;
function updateChaseCamera(bodyHeading, x, y) {
  if (!gpu || !wasmModule?.set_chase_camera_heading) return;
  const now = performance.now();
  const dt = chaseCameraLastFrameMs === null ? 0 : (now - chaseCameraLastFrameMs) / 1000;
  chaseCameraLastFrameMs = now;
  const key = `${connected ? activeRaceId : "local"}|${networkSessionRevision}|${registeredTrackFingerprint}`;
  const heading = chaseCameraSmoother.update(bodyHeading, dt, key, [x, y]);
  tickEl.dataset.cameraHeading = heading.toFixed(6);
  try {
    wasmModule.set_chase_camera_heading(heading);
  } catch (error) {
    console.warn("Could not set chase camera heading", error);
  }
}

function render() {
  const width = canvas.width, height = canvas.height;
  if (!width || !height) return;
  const s = snapshot();
  tickEl.dataset.heading = s.heading.toFixed(6);
  tickEl.dataset.x = String(s.x);
  tickEl.dataset.y = String(s.y);
  tickEl.dataset.z = String(s.z ?? 0);
  tickEl.dataset.vx = String(s.vx ?? 0);
  tickEl.dataset.vy = String(s.vy ?? 0);
  tickEl.dataset.speed = String(s.speed ?? 0);
  // Signed speed along the car's heading (negative: reversing), for tests and diagnostics.
  tickEl.dataset.forwardSpeed = String(
    (s.vx ?? s.velocity_x ?? 0) * Math.cos(s.heading) + (s.vy ?? s.velocity_y ?? 0) * Math.sin(s.heading));
  tickEl.dataset.gearCode = String(s.gear ?? "");
  tickEl.dataset.previewGrounded = previewGrounded && !connected ? "true" : "false";
  tickEl.dataset.server = connected ? "true" : "false";
  tickEl.dataset.checkpoint = String(s.checkpoint ?? (!connected && typeof race?.checkpoint === "function" ? race.checkpoint() : 0));
  tickEl.dataset.lap = String(s.lap ?? "");
  tickEl.dataset.raceLaps = String(connected ? (currentRaceSummary()?.laps ?? 0) : importedRaceLaps());
  tickEl.dataset.racePosition = String(s.race_position ?? 0);
  tickEl.dataset.vehicleCount = String(authoritative?.vehicles?.length ?? 0);
  localOpponentStates = !connected && race && typeof race.opponent_states === "function"
    ? decodeOpponentStates(race.opponent_states()) : [];
  syncProjectCarVisibility();
  tickEl.dataset.opponentCount = String(connected
    ? (authoritative?.vehicles ?? []).filter((vehicle) => vehicle.is_ai).length
    : localOpponentStates.length);
  tickEl.dataset.opponents = JSON.stringify(connected
    ? (authoritative?.vehicles ?? []).filter((vehicle) => vehicle.is_ai).map((vehicle) => ({
      id: vehicle.player_id, x: vehicle.x, y: vehicle.y, speed: vehicle.speed, laps: vehicle.completed_laps,
      finished: vehicle.finished, position: vehicle.race_position,
    }))
    : localOpponentStates.map((car) => ({ id: car.id, x: car.x, y: car.y, speed: car.speed, laps: car.laps, finished: car.finished, position: car.position })));
  tickEl.dataset.serverTick = String(authoritative?.server_tick ?? "");
  const remoteSnapshotVehicle = authoritative?.vehicles?.find((vehicle) => vehicle.player_id !== playerId);
  tickEl.dataset.remoteX = remoteSnapshotVehicle ? String(remoteSnapshotVehicle.x) : "";
  tickEl.dataset.remoteY = remoteSnapshotVehicle ? String(remoteSnapshotVehicle.y) : "";
  tickEl.dataset.finished = String(connected && !!authoritative?.vehicles?.find((v) => v.player_id === playerId)?.finished);
  tickEl.dataset.remoteVehicleCount = String(Math.max(0, (authoritative?.vehicles?.length ?? 0)
    - (authoritative?.vehicles?.some((vehicle) => vehicle.player_id === playerId) ? 1 : 0)));
  updateHud(s);
  updateAudio(s);
  drawRouteMap(s);
  computeHudActors(s);
  try {
    renderCanvasHud(s);
  } catch (error) {
    console.warn("HUD draw failed", error);
  }
  if (gpu && (race || connected)) {
    try {
      if (privateScenePose) {
        let vehicles = [];
        let localVehicleIndex = -1;
        let soloModels = false;
        const projectRoom = connected && activeRaceId !== null && activeProjectTrack?.room === true
          && currentProjectRoomTrackId() === activeProjectTrack.id;
        if (connected) {
          if (activeRaceId === null) {
            vehicles = [{ ...privateRenderPose(snapshot()), playerId: "local-lobby-preview", carPick: selectionIndices(projectCarCatalog ?? { cars: [] }, projectCarSelection) }];
            localVehicleIndex = 0;
          } else if (projectRoom) {
            const serverVehicles = authoritative?.vehicles ?? [];
            if (serverVehicles.length > 8) throw new Error("server vehicle snapshot exceeds the render limit");
            vehicles = [...serverVehicles]
              .sort((a, b) => a.player_id - b.player_id)
              .map((vehicle) => ({
                ...privateRenderPose({ ...vehicle, groundNormal: vehicle.ground_normal, absoluteZ: true }),
                playerId: vehicle.player_id,
                carPick: snapshotCarPick(vehicle),
              }));
            localVehicleIndex = vehicles.findIndex((vehicle) => vehicle.playerId === playerId);
          } else {
            const serverVehicles = authoritative?.vehicles ?? [];
            if (serverVehicles.length > 8) throw new Error("server vehicle snapshot exceeds the render limit");
            vehicles = [...serverVehicles]
              .sort((a, b) => a.player_id - b.player_id)
              .map((vehicle) => {
                // Exact paint variant when registered, else another colour of the same car.
                let shown = vehicle;
                let paintColour = resolveRenderColour(registeredVehicleProfiles, shown.vehicle_profile_id, shown.vehicle_fingerprint, paintColourFromId(shown.paint_id));
                if (paintColour === undefined && vehicle.is_ai) {
                  // A computer car whose model is not prepared here yet: draw it as the player's own car.
                  const own = authoritative?.vehicles?.find((v) => v.player_id === playerId);
                  if (own) {
                    shown = { ...vehicle, vehicle_profile_id: own.vehicle_profile_id, vehicle_fingerprint: own.vehicle_fingerprint, paint_id: own.paint_id };
                    paintColour = resolveRenderColour(registeredVehicleProfiles, shown.vehicle_profile_id, shown.vehicle_fingerprint, paintColourFromId(shown.paint_id));
                  }
                }
                if (paintColour === undefined) return null;
                return {
                  ...privateRenderPose({ ...vehicle, groundNormal: vehicle.ground_normal, absoluteZ: true }),
                  playerId: vehicle.player_id,
                  vehicle_profile_id: shown.vehicle_profile_id,
                  vehicle_source_fingerprint: shown.vehicle_fingerprint,
                  paint_colour: paintColour,
                };
              })
              .filter(Boolean);
            localVehicleIndex = vehicles.findIndex((vehicle) => vehicle.playerId === playerId);
          }
        } else {
          const heights = typeof race?.opponent_heights === "function" ? race.opponent_heights() : [];
          const roster = activeProjectTrack ? projectRoster() : null;
          // Computer opponents: the generated track's demo cars, or on an imported open-drive scene
          // the scene's own car model (the renderer draws one car body for every pose there).
          const opponentCars = activeProjectTrack || previewOpenDrive
            ? localOpponentStates.slice(0, 7).map((car, index) => ({
              ...privateRenderPose({ x: car.x, y: car.y, z: heights[index] ?? 0, heading: car.heading, groundNormal: [0, 0, 1], absoluteZ: activeProjectTrack ? true : previewGrounded }),
              playerId: `ai-${car.id}`,
              carPick: rosterPair(roster, 1 + index),
            }))
            : [];
          vehicles = [{ ...privateRenderPose({ ...s, absoluteZ: previewGrounded }), playerId: "local-practice", carPick: rosterPair(roster, 0) }, ...opponentCars];
          localVehicleIndex = 0;
          // Installed track with its car models prepared: every car is drawn with its own model.
          if (previewOpenDrive && soloRoster?.cars?.length && registeredTrackFingerprint) {
            const withModels = vehicles.map((vehicle, index) => {
              const entry = soloRoster.cars[index] ?? soloRoster.cars[0];
              const paintColour = resolveRenderColour(registeredVehicleProfiles, entry.vehicle_profile_id, entry.vehicle_source_fingerprint, entry.paint_colour);
              return paintColour === undefined ? null : { ...vehicle, ...entry, paint_colour: paintColour };
            });
            if (withModels.every(Boolean)) { vehicles = withModels; soloModels = true; }
          }
        }
        const fallbackCamera = privateRenderPose({ ...s, absoluteZ: previewGrounded && !connected });
        const localRenderVehicle = vehicles[localVehicleIndex];
        updateChaseCamera(localRenderVehicle?.heading ?? fallbackCamera.heading, localRenderVehicle?.x ?? fallbackCamera.x, localRenderVehicle?.y ?? fallbackCamera.y);
        const rosterSignature = JSON.stringify({
          raceId: connected ? activeRaceId : "local-practice",
          players: vehicles.map((vehicle) => String(vehicle.playerId)),
        });
        const summary = roomSummary();
        const useProfileRenderer = (connected && summary?.open_drive) || soloModels;
        if (connected && activeRaceId !== null && !useProfileRenderer && !projectRoom) {
          // Owner descriptors replace the generic project scene in this WASM
          // renderer. Never display that owner scene as generic room content.
          return;
        }
        if (useProfileRenderer) {
          if (!soloModels && registeredTrackFingerprint !== summary.track_fingerprint) return;
          const packedPoses = new Float32Array(vehicles.length * 7);
          vehicles.forEach((vehicle, index) => packedPoses.set([
            vehicle.x, vehicle.y, vehicle.z, vehicle.heading,
            vehicle.groundNormal?.[0] ?? 0, vehicle.groundNormal?.[1] ?? 0, vehicle.groundNormal?.[2] ?? 1,
          ], index * 7));
          const profilePairs = vehicles.map(({ vehicle_profile_id, vehicle_source_fingerprint, paint_colour }) => ({
            vehicle_profile_id,
            vehicle_source_fingerprint,
            ...(Number.isInteger(paint_colour) ? { paint_colour } : {}),
          }));
          wasmModule.render_wgpu_world_vehicles_with_profiles(
            packedPoses,
            JSON.stringify(profilePairs),
            JSON.stringify(soloModels
              ? { raceId: "local-practice", revision: privateSceneRevision, players: vehicles.map((vehicle) => String(vehicle.playerId)), models: profilePairs.map((pair) => `${pair.vehicle_profile_id}:${pair.paint_colour ?? ""}`) }
              : { raceId: activeRaceId, revision: authoritative?.selection_revision, players: vehicles.map((vehicle) => vehicle.playerId) }),
            localVehicleIndex,
            fallbackCamera.x,
            fallbackCamera.y,
            fallbackCamera.z,
            fallbackCamera.heading,
            width,
            height,
          );
          // Which car model each drawn car uses, in draw order (diagnostics and tests).
          tickEl.dataset.renderedModels = profilePairs.map((pair) => pair.vehicle_profile_id).join(",");
        } else {
          const packedPoses = new Float32Array(vehicles.length * 4);
          vehicles.forEach((vehicle, index) => packedPoses.set(
            [vehicle.x, vehicle.y, vehicle.z, vehicle.heading], index * 4,
          ));
          wasmModule.render_wgpu_world_vehicles(
            packedPoses,
            rosterSignature,
            localVehicleIndex,
            fallbackCamera.x,
            fallbackCamera.y,
            fallbackCamera.z,
            fallbackCamera.heading,
            width,
            height,
            previewGrounded && !connected,
            fallbackCamera.groundNormal?.[0] ?? 0,
            fallbackCamera.groundNormal?.[1] ?? 0,
            fallbackCamera.groundNormal?.[2] ?? 1,
            packedCarPicks(vehicles),
          );
          publishRenderedCars();
        }
      } else {
        updateChaseCamera(s.heading, s.x, s.y);
        const roomVehicles = connected && activeRaceId !== null && isCurrentRoomSnapshot(authoritative, activeRaceId) ? (authoritative?.vehicles ?? []) : [];
        const otherCars = connected
          ? roomVehicles
            .filter((vehicle) => vehicle.player_id !== playerId)
            .sort((a, b) => a.player_id - b.player_id)
            .map((vehicle) => ({ x: vehicle.x, y: vehicle.y, heading: vehicle.heading, carPick: snapshotCarPick(vehicle) }))
          : localOpponentStates;
        if (otherCars.length > 0 && typeof wasmModule.render_wgpu_project_vehicles === "function") {
          const { poses, signature, localIndex } = projectRenderPoses({ x: s.x, y: s.y, heading: s.heading }, otherCars.slice(0, 7));
          let picks = new Uint8Array(0);
          if (projectCarCatalog) {
            const ownSnapshot = roomVehicles.find((vehicle) => vehicle.player_id === playerId);
            const roster = connected ? null : projectRoster();
            const own = connected ? (snapshotCarPick(ownSnapshot) ?? selectionIndices(projectCarCatalog, projectCarSelection)) : rosterPair(roster, 0);
            picks = packPicks([own, ...otherCars.slice(0, 7).map((car, index) => (connected ? car.carPick : rosterPair(roster, 1 + index)))]);
          }
          wasmModule.render_wgpu_project_vehicles(poses, signature, localIndex, width, height, picks);
          publishRenderedCars();
        } else {
          wasmModule.render_wgpu(s.x, s.y, s.heading, s.speed, connected ? (authoritative?.vehicles?.find((v) => v.player_id === playerId)?.checkpoint ?? 0) : race.checkpoint(), width, height);
        }
      }
      gpuRenderFailures = 0;
      return;
    } catch (error) {
      // A transient GPU error (for example a lost surface while the window or
      // fullscreen size changes) must never swap the 3D view for another one:
      // keep the driving camera, reconfigure the surface and retry next frame.
      gpuRenderFailures++;
      if (gpuRenderFailures === 1) console.warn("GPU frame failed; retrying", error);
      resize();
      if (gpuRenderFailures >= 120) statusEl.textContent = "3D rendering stopped (GPU surface error) · reload to recover";
      return;
    }
  }
  if (!gpu && ctx) {
    ctx.fillStyle = "#151b1e"; ctx.fillRect(0, 0, width, height);
    ctx.fillStyle = "#dce5e8"; ctx.font = `${Math.max(14, Math.round(width / 40))}px sans-serif`; ctx.textAlign = "center";
    ctx.fillText("3D rendering is unavailable in this browser", width / 2, height / 2);
  }
}

const handleFrameFault = createFrameFaultHandler({
  releaseInputs: () => { frameFaulted = true; clearHeldInputs(); },
  releaseNetwork: () => {
    if (!connected || activeRaceId === null || !socket || socket.readyState !== WebSocket.OPEN) return;
    sendDriverInput({ throttle: 0, brake: 0, steer: 0, handbrake: false }, false);
    sendMessage("leave_race");
    try { socket.close(1000, "frame update stopped"); } catch { /* Closing is best-effort; inputs were already neutralized. */ }
  },
  report: (error) => {
  const detail = String(error?.message ?? error ?? "unknown error")
    .replace(/[\u0000-\u001f\u007f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 120);
  statusEl.textContent = "Simulation stopped · reload to recover";
  tickEl.dataset.frameFault = "true";
  frameRecoveryNotice.textContent = `The game update stopped after an internal error${detail ? ` (${detail})` : ""}. Controls were released. Reload the game to restart safely; reselect the installed folder after reload if the preview is unavailable.`;
  frameRecoveryNotice.hidden = false;
  frameRecoveryButton.hidden = false;
  },
});

function sendMessage(message) {
  if (!connected || !wasmModule?.encode_client_message) return;
  try { socket.send(wasmModule.encode_client_message(JSON.stringify(message))); }
  catch (error) { console.warn("Could not encode server message", error); }
}

/**
 * Lets a `vehicleModel` plugin send per-connection vehicle parameters (derived
 * numbers, never files) so the server can run this player's car with a matching
 * model. The plugin decides whether the host qualifies; with nothing to send this
 * does nothing.
 */
function sendVehicleModelMessage() {
  if (!connected || !race) return;
  try {
    const text = RacerPlugins.invoke("vehicleModel", "message", [{ hostname: location.hostname, race }], null);
    if (text) sendMessage(JSON.parse(text));
  } catch (error) { console.warn("Could not prepare the vehicle model parameters", error); }
}

function sendDriverInput(input, resetRequest = false) {
  const message = inputHistory.record({
    client_tick: authoritative?.server_tick ?? 0,
    throttle: input.throttle,
    brake: input.brake,
    steer: input.steer,
    handbrake: input.handbrake,
    shift_up: !!input.shift_up,
    shift_down: !!input.shift_down,
    reset_request: resetRequest,
  });
  sendMessage({ driver_input: message });
}

function unwrap(message) {
  if (typeof message === "string") return [message, {}];
  if (!message || typeof message !== "object") return ["", {}];
  const [kind] = Object.keys(message);
  return [kind, message[kind] ?? {}];
}

function connectLobby() {
  if (localPracticeMode) return;
  if (!wasmModule?.encode_client_message || !wasmModule?.decode_server_message || !("WebSocket" in window)) {
    document.querySelector("#network-status").textContent = "Server connection requires the WASM network codec.";
    return;
  }
  const url = new URL("/ws", location.href); url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
  socket = new WebSocket(url); socket.binaryType = "arraybuffer";
  socket.addEventListener("open", () => {
    releasePreviewLookBehind();
    connected = true;
    networkSessionRevision++;
    document.querySelector("#network-status").textContent = "Connected to this server";
    sendMessage({ hello: { protocol_major: 15, build_id: "0.1.0", content_capabilities: ["racer:test-circuit-v1", ...(sceneSourceHook?.contentCapabilities ?? [])] } });
    sendVehicleModelMessage();
    sendMessage(setTransmissionMessage(networkTransmission, autoReverseEnabled));
    sendMessage("list_races");
  });
  socket.addEventListener("message", (event) => {
    try {
      const decoded = wasmModule.decode_server_message(new Uint8Array(event.data));
      const [kind, body] = unwrap(JSON.parse(decoded));
      if (kind === "welcome") { playerId = body.player_id; document.querySelector("#player-id").textContent = `Driver ${playerId}`; }
      else if (kind === "lobby_snapshot") {
        latestLobby = body.races ?? [];
        latestAvailableTracks = body.available_tracks ?? [];
        updateNetworkTrackChoice();
        renderLobby();
        updateRoomCopy();
        if (activeRaceId !== null) void synchronizeRoomVehicleTemplates().catch(() => {});
      }
      else if (kind === "race_created") { releasePreviewLookBehind(); activeRaceId = body.race_id; inputHistory.reset(); readyState = false; pendingReadyRequest = null; roomTemplateSyncSignature = null; roomTemplateReadySignature = null; networkSessionRevision++; sceneSource?.refresh?.(); }
      else if (kind === "join_accepted") { releasePreviewLookBehind(); activeRaceId = body.race_id; inputHistory.reset(); roomSelectionRevision = body.selection_revision ?? 0; readyState = false; pendingReadyRequest = null; roomTemplateSyncSignature = null; roomTemplateReadySignature = null; networkSessionRevision++; sceneSource?.refresh?.(); showRoom(); }
      else if (kind === "join_rejected" || kind === "error") { roomSelectionPending = false; pendingReadyRequest = null; document.querySelector("#network-status").textContent = humanizeError(body.reason ?? body.detail ?? body.code ?? "Server rejected the request"); updateRoomSelectionControls(); updateRoomCopy(); }
      else if (kind === "left_race") { releasePreviewLookBehind(); activeRaceId = null; inputHistory.reset(); authoritative = null; readyState = false; pendingReadyRequest = null; roomSelectionPending = false; roomTemplateSyncEpoch++; roomTemplateSyncSignature = null; roomTemplateReadySignature = null; currentRoomTemplateProfiles = []; networkSessionRevision++; sceneSource?.refresh?.(); showRoom(); }
      else if (kind === "ready_accepted") {
        const summary = roomSummary();
        if (!matchesReadyAcknowledgement(body, pendingReadyRequest, activeRaceId, summary?.selection_revision)) return;
        readyState = body.ready;
        roomSelectionRevision = body.selection_revision;
        pendingReadyRequest = null;
        document.querySelector("#ready").textContent = readyState ? "Cancel ready" : "Ready";
        updateRoomCopy();
      }
      else if (kind === "countdown") {
        if (!isCurrentRaceMessage(body, activeRaceId)) return;
        document.querySelector("#room-state").textContent = `Starting · ${Math.ceil(body.ticks_left / 60)}s`;
        updateRoomCopy();
      }
      else if (kind === "state_snapshot") {
        if (activeRaceId === null || !isCurrentRoomSnapshot(body, activeRaceId)) return;
        const previousRevision = authoritative?.selection_revision;
        authoritative = body;
        roomSelectionRevision = body.selection_revision ?? roomSelectionRevision;
        if (body.selection_revision !== previousRevision) {
          inputHistory.reset();
          roomSelectionPending = false;
          if (previousRevision !== undefined) {
            readyState = false;
            document.querySelector("#ready").textContent = "Ready";
          }
        }
        const own = body.vehicles?.find((v) => v.player_id === playerId);
        if (own) {
          inputHistory.acknowledgeSnapshot(body);
        }
        document.querySelector("#room-state").textContent = own?.finished ? "Finished" : statusLabel(body.status);
        updateRoomCopy();
        const synchronization = synchronizeRoomVehicleTemplates();
        const roomWorkToken = captureRoomWorkToken();
        void synchronization.catch((error) => {
          ifCurrentRoomWork(roomWorkToken, currentRoomWorkState(), () => {
            const reason = error instanceof Error ? error.message : String(error);
            document.querySelector("#room-guidance").textContent = `Local car models are not ready: ${reason}`;
          });
        });
      }
      else if (kind === "reliable_event") {
        if (!isCurrentRaceMessage(body, activeRaceId)) return;
        tickEl.dataset.lastEvent = `${body.event}:${body.value}`;
      }
      else RacerPlugins.invoke("sceneSource", "onServerMessage", [kind, body, sceneSourceHost], false);
    } catch (error) { console.warn("Could not decode server message", error); }
  });
  socket.addEventListener("close", createLobbyCloseHandler({
    getState: () => ({
      connected,
      authoritative,
      roomSelectionPending,
      pendingReadyRequest,
      readyState,
      roomTemplateSyncEpoch,
      roomTemplateSyncSignature,
      roomTemplateReadySignature,
      currentRoomTemplateProfiles,
      networkSessionRevision,
    }),
    setState: (disconnected) => {
      connected = disconnected.connected;
      authoritative = disconnected.authoritative;
      roomSelectionPending = disconnected.roomSelectionPending;
      pendingReadyRequest = disconnected.pendingReadyRequest;
      readyState = disconnected.readyState;
      roomTemplateSyncEpoch = disconnected.roomTemplateSyncEpoch;
      roomTemplateSyncSignature = disconnected.roomTemplateSyncSignature;
      roomTemplateReadySignature = disconnected.roomTemplateReadySignature;
      currentRoomTemplateProfiles = disconnected.currentRoomTemplateProfiles;
      networkSessionRevision = disconnected.networkSessionRevision;
    },
    refreshUi: () => {
      releasePreviewLookBehind();
      document.querySelector("#network-status").textContent = "Disconnected · reload to reconnect; local practice remains available";
      updateCreateRaceAvailability();
      showRoom();
    },
  }));
  socket.addEventListener("error", () => { releasePreviewLookBehind(); document.querySelector("#network-status").textContent = "Server unavailable · local practice remains available"; });
}

function statusLabel(status) {
  if (typeof status === "string") return status[0].toUpperCase() + status.slice(1);
  if (status?.countdown) return `Starting · ${Math.ceil(status.countdown.ticks_left / 60)}s`;
  return Object.keys(status ?? {})[0]?.replace(/^./, (c) => c.toUpperCase()) ?? "Lobby";
}

function renderLobby() {
  syncProjectRoomScene();
  const list = document.querySelector("#race-list"); list.replaceChildren();
  if (!latestLobby.length) {
    const empty = document.createElement("p"); empty.className = "empty-races";
    const status = document.querySelector("#network-status").textContent;
    empty.textContent = connected ? "No open races on this host. Create a race or choose Drive solo above."
      : status.startsWith("Connecting") ? "Connecting to this host's race lobby…"
        : "Race server unavailable. Practice solo above or reconnect to host a race.";
    list.append(empty); return;
  }
  for (const item of latestLobby) {
    const row = document.createElement("div"); row.className = "race-row";
    const label = document.createElement("span"); label.textContent = item.name;
    const meta = document.createElement("span"); meta.className = "race-meta"; meta.textContent = `${item.player_count}/${item.max_players} drivers${item.ai_count > 0 ? ` + ${item.ai_count} CPU` : ""} · ${item.laps} laps · ${statusLabel(item.status)}`;
    const profile = networkSelectionForTrack(item.track_profile_id);
    const matchingTrack = item.track_fingerprint === profile.track_fingerprint;
    const button = document.createElement("button"); button.type = "button"; button.textContent = "Join";
    button.disabled = !connected || !!activeRaceId || statusLabel(item.status) !== "Lobby" || !profile.ok || !matchingTrack || !validNetworkDriverName();
    button.title = !profile.ok ? profile.reason : !matchingTrack ? "Import the matching track on this device before joining." : !validNetworkDriverName() ? "Enter a driver name up to 24 UTF-8 bytes." : "";
    button.addEventListener("click", async () => {
      const selected = networkSelectionForTrack(item.track_profile_id);
      if (!selected.ok || item.track_fingerprint !== selected.track_fingerprint) return;
      if (selected.track_profile_id === "track-profile-project-circuit"
          && !await restoreProjectSceneForGenericRoom()) return;
      const joiningGenerated = projectTrackIdFromProfile(selected.track_profile_id);
      if (joiningGenerated && !activateProjectRoomScene(joiningGenerated)) return;
      sendVehicleModelMessage();
      setNetworkTransmission(clampTransmission(document.querySelector("#race-transmission")?.value ?? networkTransmission));
      sendMessage({ join_race: {
        race_id: item.race_id,
        content_identity: selected.content_identity,
        track_profile_id: selected.track_profile_id,
        track_fingerprint: selected.track_fingerprint,
        vehicle_profile_id: selected.vehicle_profile_id,
        vehicle_fingerprint: selected.vehicle_fingerprint,
        driver_name: networkDriverName.value.trim(),
        paint_id: networkPaintChoice.value,
        ...wireFields(projectCarCatalog, projectCarSelection),
      } });
    });
    row.append(label, meta, button); list.append(row);
  }
}

function updateNetworkTrackChoice() {
  if (!networkTrackChoice) return;
  const previous = networkTrackChoice.value;
  networkTrackChoice.replaceChildren();
  const projectOption = document.createElement("option");
  projectOption.value = "track-profile-project-circuit";
  projectOption.textContent = "Test Circuit · project content";
  networkTrackChoice.append(projectOption);
  for (const generated of projectTrackCatalog) {
    const option = document.createElement("option");
    option.value = generated.profileId;
    option.textContent = `${generated.name} · generated track`;
    networkTrackChoice.append(option);
  }
  for (const track of [...latestAvailableTracks].sort((a, b) => a.profile_id.localeCompare(b.profile_id))) {
    const option = document.createElement("option");
    option.value = track.profile_id;
    option.textContent = sceneSource?.trackLabel?.(track.profile_id) ?? track.profile_id;
    networkTrackChoice.append(option);
  }
  // New rooms default to the scenic track (same rule as the lobby preview); automation keeps the test circuit.
  const scenic = menuPreviewWanted() ? projectTrackCatalog.find((track) => track.id === MENU_PREVIEW_TRACK) : null;
  const keepPrevious = networkTrackPickedByUser || previous !== "track-profile-project-circuit";
  networkTrackChoice.value = keepPrevious && [...networkTrackChoice.options].some((option) => option.value === previous)
    ? previous
    : scenic?.profileId ?? "track-profile-project-circuit";
  updateCreateRaceAvailability();
}

function networkSelectionForTrack(trackProfileId) {
  const generated = projectTrackCatalog.find((track) => track.profileId === trackProfileId);
  if (generated) {
    return {
      ok: true,
      content_identity: PROJECT_TRACK_CONTENT_ID,
      track_profile_id: generated.profileId,
      track_fingerprint: generated.fingerprint,
      vehicle_profile_id: "vehicle-profile-project-default",
      vehicle_fingerprint: "0".repeat(64),
    };
  }
  if (trackProfileId === "track-profile-project-circuit") {
    return {
      ok: true,
      content_identity: "racer:test-circuit-v1",
      track_profile_id: trackProfileId,
      track_fingerprint: "0".repeat(64),
      vehicle_profile_id: "vehicle-profile-project-default",
      vehicle_fingerprint: "0".repeat(64),
    };
  }
  return sceneSource?.selectionForTrack?.(trackProfileId, latestAvailableTracks)
    ?? { ok: false, reason: "This track needs a scene source plugin that is not loaded." };
}

async function restoreProjectSceneForGenericRoom() {
  if (!privateScenePose) return true;
  if (typeof wasmModule?.restore_project_scene !== "function") {
    document.querySelector("#network-status").textContent = "This renderer cannot restore project content yet. Reload without an imported scene before joining this room.";
    return false;
  }
  try {
    wasmModule.restore_project_scene();
    race?.use_project_circuit();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    document.querySelector("#network-status").textContent = `Could not restore the project race scene: ${detail}`;
    return false;
  }
  privateScenePose = null;
  activeProjectTrack = null;
  delete tickEl.dataset.projectTrack;
  applyRouteMap(null);
  registeredVehicleProfiles.clear();
  registeredTrackFingerprint = null;
  activeSceneTrackFingerprint = null;
  currentRoomTemplateProfiles = [];
  roomTemplateSyncSignature = null;
  roomTemplateReadySignature = null;
  roomTemplateSyncError = null;
  roomTemplateFailedSignature = null;
  document.querySelector("#mode-label").textContent = "Project-owned multiplayer preview";
  sceneSource?.notice?.("Project-owned race scene restored. Saved previews remain in this browser.");
  return true;
}

function updateCreateRaceAvailability() {
  const createButton = document.querySelector("#create-race-form button[type='submit']");
  if (!createButton) return;
  const profile = networkSelectionForTrack(networkTrackChoice.value);
  createButton.disabled = !connected || !profile.ok || !validNetworkDriverName();
  createButton.title = profile.ok ? "" : profile.reason;
  const pluginTrackChoice = networkTrackChoice.value !== "track-profile-project-circuit"
    && !projectTrackIdFromProfile(networkTrackChoice.value);
  // Installed tracks race when the server finds a course in their road; "Free drive" skips laps and opponents.
  const lapsSelect = document.querySelector("#race-laps");
  const freeDriveOption = lapsSelect.querySelector("option[value='0']");
  if (freeDriveOption) { freeDriveOption.hidden = !pluginTrackChoice; freeDriveOption.disabled = !pluginTrackChoice; }
  if (!pluginTrackChoice && lapsSelect.value === "0") lapsSelect.value = "2";
  lapsSelect.title = pluginTrackChoice ? "Laps on this installed track; Free drive has no laps or opponents." : "";
}

function validNetworkDriverName() {
  const value = networkDriverName.value.trim();
  return value.length > 0 && new TextEncoder().encode(value).length <= 24 && ![...value].some((char) => /[\u0000-\u001f\u007f]/u.test(char));
}

function showRoom() {
  const inRoom = activeRaceId !== null;
  document.querySelector("#room-controls").hidden = !inRoom;
  document.querySelector("#create-race-form").hidden = inRoom;
  document.querySelector("#room-title").textContent = latestLobby.find((item) => item.race_id === activeRaceId)?.name ?? "Race joined";
  renderLobby();
  updateRoomCopy();
}

function updateRoomSelectionControls() {
  if (!roomTrackChoice || !roomVehicleChoice) return;
  const summary = roomSummary();
  const inOpenDrive = activeRaceId !== null && summary?.open_drive === true;
  const inLobby = inOpenDrive && statusLabel(summary.status) === "Lobby";
  const revisionSynchronized = authoritative?.selection_revision === summary?.selection_revision;
  const isHost = summary?.host_player_id === playerId;
  const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  const trackWrap = document.querySelector("#room-track-choice-wrap");
  const carWrap = document.querySelector("#room-vehicle-choice-wrap");
  const paintWrap = document.querySelector("#room-paint-choice-wrap");
  for (const wrap of [trackWrap, carWrap, paintWrap]) wrap.hidden = !inOpenDrive;
  const lapsWrap = document.querySelector("#room-laps-wrap");
  if (lapsWrap) lapsWrap.hidden = !(inOpenDrive && isHost && inLobby);
  if (roomLapsChoice && inOpenDrive) {
    if (document.activeElement !== roomLapsChoice) roomLapsChoice.value = String(summary.laps ?? 0);
    roomLapsChoice.disabled = !connected || !inLobby || !isHost || !revisionSynchronized || roomSelectionPending;
  }
  if (!inOpenDrive) return;

  const trackOptions = [...latestAvailableTracks]
    .sort((a, b) => a.profile_id.localeCompare(b.profile_id))
    .map((track) => ({
      value: track.profile_id,
      label: sceneSource?.roomTrackLabel?.(track.profile_id) ?? track.profile_id,
    }));
  if (!trackOptions.some((option) => option.value === summary.track_profile_id)) {
    trackOptions.push({
      value: summary.track_profile_id,
      label: `${summary.track_profile_id} · unavailable on this server`,
    });
  }
  reconcileSelectOptions(
    roomTrackChoice,
    trackOptions,
    summary.track_profile_id,
    !connected || !inLobby || !isHost || !revisionSynchronized || sceneSource?.loading || roomSelectionPending,
    ({ value, label, disabled }) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      option.disabled = !!disabled;
      return option;
    },
  );

  const catalogTrack = latestAvailableTracks.find((track) => track.profile_id === summary.track_profile_id);
  const carOptions = [...(catalogTrack?.cars ?? [])]
    .sort((a, b) => a.profile_id.localeCompare(b.profile_id))
    .map((car) => ({
      value: car.profile_id,
      label: sceneSource?.vehicleLabel?.(car.profile_id) ?? car.profile_id,
    }));
  if (own && !carOptions.some((option) => option.value === own.vehicle_profile_id)) {
    carOptions.push({
      value: own.vehicle_profile_id,
      label: `${own.vehicle_profile_id} · unsupported for selected track`,
      disabled: true,
    });
  }
  const selectedCar = carOptions.some((option) => option.value === own?.vehicle_profile_id)
    ? own.vehicle_profile_id
    : roomVehicleChoice.value && carOptions.some((option) => option.value === roomVehicleChoice.value)
      ? roomVehicleChoice.value
      : carOptions[0]?.value ?? "";
  reconcileSelectOptions(
    roomVehicleChoice,
    carOptions,
    selectedCar,
    !connected || !inLobby || !own || !revisionSynchronized || sceneSource?.loading || roomSelectionPending || carOptions.length < 2,
    ({ value, label, disabled }) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      option.disabled = !!disabled;
      return option;
    },
  );
  populatePaintSelect(roomPaintChoice, paintColourFromId(own?.paint_id) ?? effectivePaintColour());
  roomPaintChoice.disabled = !connected || !inLobby || !own || !revisionSynchronized || roomSelectionPending;
}

let roomSelectionPending = false;

/** Room gearbox: remember, tell the server (stored for the room this player creates or joins) and the local simulation. */
function setNetworkTransmission(value) {
  networkTransmission = clampTransmission(value);
  for (const id of ["#race-transmission", "#room-transmission"]) {
    const el = document.querySelector(id);
    if (el && el.value !== networkTransmission) el.value = networkTransmission;
  }
  sendMessage(setTransmissionMessage(networkTransmission, autoReverseEnabled));
  applyTransmissionToRace();
  refreshControlsHint();
}

function updateRoomTransmissionControl() {
  const wrap = document.querySelector("#room-transmission-wrap");
  const select = document.querySelector("#room-transmission");
  if (!wrap || !select) return;
  const summary = roomSummary();
  const inLobby = activeRaceId !== null && summary && statusLabel(summary.status) === "Lobby";
  wrap.hidden = activeRaceId === null;
  if (document.activeElement !== select) select.value = networkTransmission;
  select.disabled = !connected || !inLobby;
  select.title = transmissionSummary(networkTransmission);
}
for (const id of ["#race-transmission", "#room-transmission"]) {
  document.querySelector(id)?.addEventListener("change", (event) => setNetworkTransmission(event.target.value));
}

function updateRoomAiControls() {
  const wrap = document.querySelector("#room-ai-wrap");
  if (!wrap) return;
  const summary = roomSummary();
  const isHost = summary?.host_player_id === playerId;
  const inLobby = summary && statusLabel(summary.status) === "Lobby";
  const racing = summary && (!summary.open_drive || summary.laps > 0);
  wrap.hidden = !(activeRaceId !== null && summary && racing && isHost && inLobby);
  if (wrap.hidden || !summary) return;
  const count = document.querySelector("#room-ai-count");
  const skill = document.querySelector("#room-ai-skill");
  const catchUp = document.querySelector("#room-ai-catchup");
  if (document.activeElement !== count) count.value = String(summary.ai_count ?? 0);
  if (document.activeElement !== skill) skill.value = String(summary.ai_skill ?? 1);
  catchUp.checked = summary.ai_catch_up !== false;
  const synchronized = authoritative?.selection_revision === summary.selection_revision;
  for (const el of [count, skill, catchUp]) el.disabled = !connected || !synchronized || roomSelectionPending;
}

function sendRoomAiChange() {
  const summary = roomSummary();
  if (!summary || summary.host_player_id !== playerId || (summary.open_drive && !(summary.laps > 0))) return;
  sendRoomSelectionChange({ configure_ai: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    ai_count: clampOpponents(document.querySelector("#room-ai-count").value, 0),
    ai_skill: clampSkill(document.querySelector("#room-ai-skill").value),
    ai_catch_up: document.querySelector("#room-ai-catchup").checked,
  } });
}
for (const id of ["#room-ai-count", "#room-ai-skill", "#room-ai-catchup"]) {
  document.querySelector(id)?.addEventListener("change", sendRoomAiChange);
}

// Solo set-up: the entry link carries the settings; changing them in a running
// practice session reloads it with the new settings.
function currentSoloSettings(prefix) {
  return {
    count: clampOpponents(document.querySelector(`#${prefix}-ai-count`).value),
    skill: clampSkill(document.querySelector(`#${prefix}-ai-skill`).value),
    catchUp: document.querySelector(`#${prefix}-ai-catchup`).checked,
    track: document.querySelector(prefix === "solo" ? "#solo-track-choice" : "#solo-track-switch")?.value || null,
    transmission: clampTransmission(document.querySelector(`#${prefix}-transmission`)?.value),
    laps: clampLaps(document.querySelector(`#${prefix}-laps`)?.value),
  };
}
function syncSoloEntryLink() {
  const link = document.querySelector("#solo-drive-link");
  if (link) link.setAttribute("href", `?${soloQuery(currentSoloSettings("solo"))}`);
}
for (const id of ["#solo-ai-count", "#solo-ai-skill", "#solo-ai-catchup", "#solo-transmission", "#solo-laps"]) {
  document.querySelector(id)?.addEventListener("change", syncSoloEntryLink);
}
syncSoloEntryLink();
if (localPracticeMode) {
  const setup = document.querySelector("#practice-ai-setup");
  const privateScene = privateSceneRequested;
  // The opponent controls do not apply to a private scene; the gearbox does.
  if (setup && privateScene) {
    for (const child of setup.children) child.hidden = !child.contains(document.querySelector("#practice-transmission"));
  }
  document.querySelector("#practice-transmission").value = soloTransmission;
  document.querySelector("#practice-ai-count").value = String(soloOpponents.count);
  document.querySelector("#practice-ai-skill").value = String(soloOpponents.skill);
  document.querySelector("#practice-ai-catchup").checked = soloOpponents.catchUp;
  document.querySelector("#practice-laps").value = String(soloOpponents.laps ?? 1);
  const note = document.querySelector("#practice-ai-note");
  if (note) note.textContent = `${opponentSummary(soloOpponents)}. Changing these restarts the session.`;
  for (const id of ["#practice-ai-count", "#practice-ai-skill", "#practice-ai-catchup", "#practice-laps"]) {
    document.querySelector(id)?.addEventListener("change", () => {
      location.search = soloQuery(currentSoloSettings("practice"));
    });
  }
  // The gearbox applies at once (the drivetrain restarts in its start gear);
  // the address keeps the choice so a reload or a shared link does too.
  document.querySelector("#practice-transmission")?.addEventListener("change", (event) => {
    soloTransmission = clampTransmission(event.target.value);
    applyTransmissionToRace();
    refreshControlsHint();
    const params = new URLSearchParams(location.search);
    if (isManual(soloTransmission)) params.set("gearbox", "manual"); else params.delete("gearbox");
    try { history.replaceState(null, "", `?${params.toString()}`); } catch { /* not critical */ }
  });
}

function sendRoomSelectionChange(message) {
  if (!connected || roomSelectionPending || activeRaceId === null) return;
  const summary = roomSummary();
  if (!summary || authoritative?.selection_revision !== summary.selection_revision
      || message.change_track?.selection_revision !== undefined && message.change_track.selection_revision !== summary.selection_revision
      || message.change_vehicle?.selection_revision !== undefined && message.change_vehicle.selection_revision !== summary.selection_revision
      || message.configure_ai?.selection_revision !== undefined && message.configure_ai.selection_revision !== summary.selection_revision) {
    updateRoomSelectionControls();
    return;
  }
  roomSelectionPending = true;
  updateRoomSelectionControls();
  sendMessage(message);
}

function updateRoomCopy() {
  const guidance = document.querySelector("#room-guidance");
  if (!guidance || activeRaceId === null) return;
  const summary = latestLobby.find((item) => item.race_id === activeRaceId);
  const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  const status = own?.finished ? "Finished" : statusLabel(authoritative?.status ?? summary?.status ?? "lobby");
  document.querySelector("#room-title").textContent = summary?.name ?? "Race joined";
  if (!roomRosterEl) {
    roomRosterEl = document.createElement("p");
    roomRosterEl.className = "room-roster";
    roomRosterEl.setAttribute("aria-live", "polite");
    document.querySelector(".room-summary")?.append(roomRosterEl);
  }
  const roster = authoritative?.vehicles ?? [];
  roomRosterEl.textContent = roster.length
    ? `Drivers: ${roster.map((vehicle) => `${vehicle.driver_name} · ${vehicle.vehicle_profile_id}`).join(", ")}`
    : "Driver names appear when the server sends the first race snapshot.";
  const roomState = document.querySelector("#room-state");
  roomState.textContent = status;
  roomState.dataset.selectionRevision = String(summary?.selection_revision ?? "");
  roomState.dataset.hostPlayerId = String(summary?.host_player_id ?? "");
  updateRoomSelectionControls();
  updateRoomAiControls();
  updateRoomTransmissionControl();
  const connectionUi = roomConnectionUiState(connected);
  document.querySelector("#ready").disabled = connectionUi.actionsDisabled || status === "Finished" || status === "Racing" || status.startsWith("Starting") || roomSelectionPending
    // The click handler ignores a Ready press while the snapshot's selection revision lags the lobby summary
    // (e.g. just after another driver joins), so keep the button disabled until they agree instead of dropping the press.
    || (summary !== undefined && authoritative?.selection_revision !== summary.selection_revision);
  document.querySelector("#leave-race").disabled = connectionUi.actionsDisabled;
  if (connectionUi.guidance) {
    guidance.textContent = connectionUi.guidance;
    return;
  }
  if (status.startsWith("Starting")) {
    guidance.textContent = "Get set. The server countdown will start the race when it reaches zero.";
    return;
  }
  if (status === "Racing") {
    guidance.textContent = "Race is live. Drive with W/A/S/D or the arrow keys; the server records laps and finish state.";
    return;
  }
  if (status === "Finished") {
    guidance.textContent = "Your server-reported race is finished. Leave the room or start another race.";
    return;
  }
  const count = summary?.player_count ?? authoritative?.vehicles?.length ?? 1;
  if (!readyState && summary?.open_drive && roomTemplateSyncError) {
    guidance.textContent = `Local car models are not ready: ${roomTemplateSyncError}`;
    return;
  }
  if (!readyState && summary?.open_drive && roomTemplateReadySignature !== roomTemplateSyncSignature) {
    guidance.textContent = "Preparing locally saved car models for every driver in this room…";
    return;
  }
  if (count + ((summary?.open_drive && !(summary?.laps > 0)) ? 0 : (summary?.ai_count ?? 0)) < 2) {
    guidance.textContent = readyState
      ? `You're ready. ${count} driver here; waiting for another participant before the server can start.`
      : `${count} of ${summary?.max_players ?? 8} drivers here. Waiting for at least one more participant; then ready up to start.`;
  } else if (!readyState) {
    guidance.textContent = `${count} drivers are here. Press Ready when you're set; the server starts the countdown when every driver is ready.`;
  } else {
    guidance.textContent = "You're ready. Waiting for the other drivers to ready up; the server starts the countdown when everyone is ready.";
  }
}

networkTrackChoice.addEventListener("change", updateCreateRaceAvailability);
networkDriverName.addEventListener("input", () => {
  updateCreateRaceAvailability();
  renderLobby();
});
networkPaintChoice.addEventListener("change", () => {
  const chosen = paintColourFromId(networkPaintChoice.value);
  if (chosen !== null) {
    paintColour = chosen;
    sceneSource?.paintPicked?.(chosen);
  }
  updateCreateRaceAvailability();
});
roomTrackChoice.addEventListener("change", () => {
  const summary = roomSummary();
  const track = latestAvailableTracks.find((item) => item.profile_id === roomTrackChoice.value);
  if (!summary?.open_drive || summary.host_player_id !== playerId || !track) { updateRoomSelectionControls(); return; }
  sendRoomSelectionChange({ change_track: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    content_identity: sceneSource?.contentIdentity ?? sceneSourceHook?.contentIdentity,
    track_profile_id: track.profile_id,
    track_fingerprint: track.source_fingerprint,
    // Keep the lap count; an unraced (free drive) track stays free drive.
    laps: summary.laps ?? 0,
  } });
});
roomLapsChoice?.addEventListener("change", () => {
  const summary = roomSummary();
  const track = latestAvailableTracks.find((item) => item.profile_id === summary?.track_profile_id);
  if (!summary?.open_drive || summary.host_player_id !== playerId || !track) { updateRoomSelectionControls(); return; }
  sendRoomSelectionChange({ change_track: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    content_identity: sceneSource?.contentIdentity ?? sceneSourceHook?.contentIdentity,
    track_profile_id: track.profile_id,
    track_fingerprint: track.source_fingerprint,
    laps: clampLaps(roomLapsChoice.value, 0),
  } });
});
roomVehicleChoice.addEventListener("change", () => {
  const summary = roomSummary();
  const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  const track = latestAvailableTracks.find((item) => item.profile_id === summary?.track_profile_id);
  const car = track?.cars?.find((item) => item.profile_id === roomVehicleChoice.value);
  if (!summary?.open_drive || !own || !car) { updateRoomSelectionControls(); return; }
  sendRoomSelectionChange({ change_vehicle: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    vehicle_profile_id: car.profile_id,
    vehicle_fingerprint: car.source_fingerprint,
    paint_id: roomPaintChoice.value,
  } });
});
roomPaintChoice.addEventListener("change", () => {
  const chosen = paintColourFromId(roomPaintChoice.value);
  if (chosen !== null) paintColour = chosen;
  const summary = roomSummary();
  const own = authoritative?.vehicles?.find((vehicle) => vehicle.player_id === playerId);
  if (!summary?.open_drive || !own) return;
  const track = latestAvailableTracks.find((item) => item.profile_id === summary.track_profile_id);
  const car = track?.cars?.find((item) => item.profile_id === own.vehicle_profile_id);
  if (!car) return;
  sendRoomSelectionChange({ change_vehicle: {
    race_id: activeRaceId,
    selection_revision: summary.selection_revision,
    vehicle_profile_id: car.profile_id,
    vehicle_fingerprint: car.source_fingerprint,
    paint_id: roomPaintChoice.value,
  } });
});

function humanizeError(message) {
  const explanations = {
    race_not_found: "That race has closed. Choose another race from the lobby.",
    race_started: "That race has already started. Choose an open lobby.",
    room_full: "That race is full. Choose another lobby.",
    already_joined: "Leave your current room before joining another.",
    content_mismatch: "This race needs different project-owned content.",
    not_in_race: "Join a race before changing readiness.",
    host_only: "Only the room host can change its track.",
    invalid_settings: "Those race settings are not valid. Check the opponents, difficulty and track.",
    stale_revision: "The room changed while that choice was being sent. Review the current room selections and try again.",
    car_unavailable_on_track: "That car is not available for the selected track.",
    profile_unavailable: "The server does not have that gameplay profile.",
    render_profile_missing: "Prepare the exact local car models for every driver before readying up.",
  };
  return explanations[message] ?? message;
}

document.querySelector("#create-race-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!connected) {
    document.querySelector("#network-status").textContent = "Disconnected · reload to reconnect before creating a race";
    updateCreateRaceAvailability();
    return;
  }
  const trackProfileId = networkTrackChoice.value;
  const profile = networkSelectionForTrack(trackProfileId);
  if (!profile.ok || !validNetworkDriverName()) {
    document.querySelector("#network-status").textContent = profile.ok
      ? "Enter a driver name up to 24 UTF-8 bytes."
      : profile.reason;
    return;
  }
  if (trackProfileId === "track-profile-project-circuit"
      && !await restoreProjectSceneForGenericRoom()) return;
  const creatingGenerated = projectTrackIdFromProfile(trackProfileId);
  if (creatingGenerated && !activateProjectRoomScene(creatingGenerated)) return;
  const { ok: _available, ...selection } = profile;
  sendVehicleModelMessage();
  setNetworkTransmission(clampTransmission(document.querySelector("#race-transmission")?.value ?? networkTransmission));
  sendMessage({ create_race: {
    name: document.querySelector("#race-name").value.trim(),
    laps: Number(document.querySelector("#race-laps").value),
    ...selection,
    driver_name: networkDriverName.value.trim(),
    paint_id: networkPaintChoice.value,
    ...wireFields(projectCarCatalog, projectCarSelection),
    ai_count: Number(document.querySelector("#race-laps").value) > 0 ? clampOpponents(document.querySelector("#race-ai-count").value) : 0,
    ai_skill: clampSkill(document.querySelector("#race-ai-skill").value),
    ai_catch_up: document.querySelector("#race-ai-catchup").checked,
  } });
});
document.querySelector("#ready").addEventListener("click", async () => {
  const summary = roomSummary();
  if (!summary) return;
  if (authoritative?.selection_revision !== summary.selection_revision) {
    document.querySelector("#room-guidance").textContent = "Waiting for the latest room revision before changing readiness.";
    return;
  }
  const nextReady = !readyState;
  let vehicleTemplates = [{ profile_id: "vehicle-profile-project-default", source_fingerprint: "0".repeat(64) }];
  let roomWorkToken = null;
  if (nextReady && summary.open_drive) {
    document.querySelector("#ready").disabled = true;
    try {
      const synchronization = synchronizeRoomVehicleTemplates();
      roomWorkToken = captureRoomWorkToken();
      vehicleTemplates = await synchronization;
      if (!isCurrentRoomWorkToken(roomWorkToken)) {
        updateRoomCopy();
        return;
      }
      if (!vehicleTemplates.length || roomTemplateSyncError) throw new Error(roomTemplateSyncError ?? "The room car roster is not ready locally");
    } catch (error) {
      if (roomWorkToken && !isCurrentRoomWorkToken(roomWorkToken)) {
        updateRoomCopy();
        return;
      }
      const reason = error instanceof Error ? error.message : String(error);
      document.querySelector("#room-guidance").textContent = `Cannot ready yet: ${reason}`;
      updateRoomCopy();
      return;
    }
  }
  if (activeRaceId !== summary.race_id
      || authoritative?.selection_revision !== summary.selection_revision
      || roomSummary()?.selection_revision !== summary.selection_revision) {
    updateRoomCopy();
    return;
  }
  const requestRevision = authoritative?.selection_revision ?? summary.selection_revision;
  pendingReadyRequest = { raceId: activeRaceId, revision: requestRevision, ready: nextReady };
  sendMessage({ ready_state: {
    ready: nextReady,
    selection_revision: requestRevision,
    track_fingerprint: summary.track_fingerprint,
    vehicle_templates: nextReady ? vehicleTemplates : [],
  } });
});
document.querySelector("#leave-race").addEventListener("click", () => { gameAudio.silence(); sendMessage("leave_race"); });
showRoom();
updateNetworkTrackChoice();

function reset() {
  if (frameFaulted) return;
  impactDetector.reset();
  if (connected) sendDriverInput({ throttle: 0, brake: 0, steer: 0, handbrake: false }, true);
  else if (race) race.request_reset();
  else fallback = { ...fallback, x: -20, y: 0, heading: 0, vx: 0, vy: 0 };
  toast.classList.remove("hidden"); toast.textContent = "Car reset"; window.setTimeout(() => toast.classList.add("hidden"), 1200);
}

window.addEventListener("keydown", (event) => {
  if (frameFaulted) return;
  const editableTarget = isEditableTarget(event.target);
  if (editableTarget) return;
  const hasModifier = event.ctrlKey || event.metaKey || event.altKey;
  const nextMirrorVisible = mirrorVisibilityAfterKeydown(previewMirrorVisible, event, editableTarget);
  if (nextMirrorVisible !== null) {
    event.preventDefault();
    setPreviewMirrorVisible(nextMirrorVisible);
    return;
  }
  if (gpu && !hasModifier && !event.repeat && event.code === "KeyC") {
    cyclePreviewCamera();
    return;
  }
  if (gpu && !hasModifier && !event.repeat && event.code === "KeyB") setPreviewLookBehind(true);
  if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space"].includes(event.code)) event.preventDefault();
  keys.add(event.code);
  if (!event.repeat && !hasModifier) {
    if (event.code === "KeyA") shiftLatch.press("up");
    else if (event.code === "KeyZ") shiftLatch.press("down");
  }
  if (event.code === "KeyR" && !event.repeat) reset();
  toast.classList.add("hidden");
});
window.addEventListener("keyup", (event) => {
  keys.delete(event.code);
  if (event.code === "KeyB") setPreviewLookBehind(false);
});
function clearHeldInputs() {
  keys.clear();
  shiftLatch.clear();
  setPreviewLookBehind(false);
}
window.addEventListener("blur", clearHeldInputs);
document.addEventListener("visibilitychange", () => {
  if (document.hidden) clearHeldInputs();
});
canvas.addEventListener("pointerdown", () => canvas.focus());
// A control that keeps focus after a mouse choice would swallow the driving keys
// (arrows change a focused select, Space presses a focused button). Keyboard
// users keep their focus.
let lastInteractionWasPointer = false;
window.addEventListener("pointerdown", () => { lastInteractionWasPointer = true; }, true);
window.addEventListener("keydown", (event) => { if (event.code === "Tab") lastInteractionWasPointer = false; }, true);
document.addEventListener("change", (event) => {
  const target = event.target;
  const chosen = target instanceof HTMLSelectElement
    || (target instanceof HTMLInputElement && ["checkbox", "radio", "range"].includes(target.type));
  if (chosen && lastInteractionWasPointer) target.blur();
});
document.addEventListener("click", (event) => {
  const button = event.target instanceof Element ? event.target.closest("button") : null;
  if (button && lastInteractionWasPointer) button.blur();
});
document.querySelector("#reset").addEventListener("click", reset);
window.addEventListener("resize", resize);
resize();
createFrameLoop({ fixedStep, render, stepSeconds, onFault: handleFrameFault }).start();
