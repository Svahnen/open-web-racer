const game = document.querySelector("#game");
const toggle = document.querySelector("#fullscreen-toggle");
const exitButton = document.querySelector("#fullscreen-exit");
const canvas = document.querySelector("#race-canvas");
const status = document.querySelector("#fullscreen-status");

function signalLayoutChange() {
  window.dispatchEvent(new Event("resize"));
}

function setFullscreenState(clearStatus = true) {
  const active = document.fullscreenElement === game;
  toggle.textContent = active ? "Exit fullscreen" : "Enter fullscreen";
  toggle.setAttribute("aria-label", active ? "Exit fullscreen game view" : "Enter fullscreen game view");
  toggle.setAttribute("aria-pressed", String(active));
  exitButton.hidden = !active;
  if (clearStatus) {
    status.hidden = true;
    status.textContent = "";
  }
  signalLayoutChange();
  const activeCanvas = document.querySelector(".game canvas.render-surface:not([hidden])") ?? canvas;
  if (active) {
    activeCanvas?.focus({ preventScroll: true });
  } else {
    // The app releases held keys on blur; Escape does not always blur the page.
    window.dispatchEvent(new Event("blur"));
    activeCanvas?.focus({ preventScroll: true });
  }
}

function showFullscreenError() {
  status.textContent = "Fullscreen could not start. Use your browser's fullscreen controls or keep driving here.";
  status.hidden = false;
  setFullscreenState(false);
}

async function toggleFullscreen() {
  status.hidden = true;
  try {
    if (document.fullscreenElement === game) {
      await document.exitFullscreen();
    } else {
      await game.requestFullscreen({ navigationUI: "hide" });
    }
  } catch {
    showFullscreenError();
  }
}

if (!game || !toggle || !exitButton || !canvas || !status) {
  throw new Error("Fullscreen game controls are missing from the page");
}

if (document.fullscreenEnabled === false || typeof game.requestFullscreen !== "function") {
  toggle.disabled = true;
  status.hidden = false;
  status.textContent = "Fullscreen is unavailable in this browser.";
} else {
  toggle.addEventListener("click", toggleFullscreen);
  exitButton.addEventListener("click", toggleFullscreen);
  document.addEventListener("fullscreenchange", setFullscreenState);
  document.addEventListener("fullscreenerror", showFullscreenError);
  window.addEventListener("keydown", (event) => {
    if (event.code === "Escape" && document.fullscreenElement === game) {
      event.preventDefault();
      void document.exitFullscreen().catch(showFullscreenError);
    }
  });
}

// Fullscreen changes the game's CSS size without changing page chrome. Resize
// the backing buffer from the current box and DPR through bootstrap's handler.
if (typeof ResizeObserver === "function") {
  const observer = new ResizeObserver(signalLayoutChange);
  observer.observe(game);
}

let dprQuery;
function watchDevicePixelRatio() {
  if (dprQuery) {
    if (dprQuery.removeEventListener) dprQuery.removeEventListener("change", onDprChange);
    else dprQuery.removeListener?.(onDprChange);
  }
  dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  if (dprQuery.addEventListener) dprQuery.addEventListener("change", onDprChange, { once: true });
  else dprQuery.addListener?.(onDprChange);
}
function onDprChange() {
  signalLayoutChange();
  watchDevicePixelRatio();
}
watchDevicePixelRatio();
