import { expect, test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// Generated project tracks (racer-trackgen): menu, selection, driving, screenshots and fps.
// Screenshots (640 px wide, chase camera, HUD hidden) go to RACER_SHOT_DIR
// (default test-results/track-screenshots) from the Chromium project only.

// The canvas HUD comes from an optional plugin; without one the card HUD shows the lap.
async function expectLap(page, lap, total) {
  const canvasHud = await page.evaluate(() => window.RacerPlugins.providers("hud").length > 0);
  if (canvasHud) await expect(page.locator("#hud-canvas")).toHaveAttribute("data-lap", `${lap}/${total}`);
  else {
    await expect(page.locator("#lap")).toHaveText(String(lap));
    await expect(page.locator("#lap-target")).toContainText(String(total));
  }
}

const TRACKS = ["coastline-run", "pine-ridge", "red-mesa", "night-harbour"];
const SPOTS = [0, 30, 62];
const shotDir = process.env.RACER_SHOT_DIR ?? join("test-results", "track-screenshots");

async function waitForTrack(page, id) {
  await expect(page.locator("#engine-status")).toContainText(/wgpu (WebGPU|WebGL2)/, { timeout: 60_000 });
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", id, { timeout: 90_000 });
  await expect.poll(async () => Number(await page.locator("#tick").textContent()), { timeout: 60_000 }).toBeGreaterThan(5);
}

async function fixCanvasSize(page) {
  await page.addStyleTag({
    content: "canvas.render-surface { width: 640px !important; height: 400px !important; min-height: 0 !important; } .game { width: 640px; } .hud, .hud-race, .toast, .route-map-panel, .hud-canvas { visibility: hidden !important; }",
  });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
  await expect.poll(async () => page.locator("canvas.render-surface:visible").evaluate((element) => element.width)).toBe(640);
}

test("solo track menu lists the generated tracks and drives the selected one", async ({ page }) => {
  test.setTimeout(240_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  const choice = page.locator("#solo-track-choice");
  await expect(choice.locator("option")).toHaveCount(5, { timeout: 60_000 });
  await expect(choice.locator("option").nth(0)).toContainText("Test Circuit");
  await choice.selectOption("pine-ridge");
  await expect(page.locator("#solo-drive-link")).toHaveAttribute("href", "?practice=local&ai=7&skill=1&catchup=1&track=pine-ridge");
  await page.locator("#solo-drive-link").click();
  await waitForTrack(page, "pine-ridge");
  await expect(page.locator("#mode-label")).toContainText("Pine Ridge");
  await expectLap(page, 1, 3);
  await expect(page.locator("#solo-track-switch")).toHaveValue("pine-ridge");
  const tick = page.locator("#tick");
  const startX = Number(await tick.getAttribute("data-x"));
  await page.locator("canvas.render-surface:visible").click();
  await page.keyboard.down("w");
  await expect.poll(async () => Number(await page.locator("#speed").textContent()), { timeout: 60_000 }).toBeGreaterThan(20);
  await expect.poll(async () => Number(await tick.getAttribute("data-x")), { timeout: 60_000 }).toBeGreaterThan(startX + 15);
  await page.keyboard.up("w");
  // grounded on the generated road
  await expect(tick).toHaveAttribute("data-preview-grounded", "true");
  expect(Number(await tick.getAttribute("data-z"))).toBeGreaterThan(5);
  expect(errors).toEqual([]);
});

test("the lobby preview shows Pine Ridge on request and gives way to the circuit in a room", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the live room automation runs once");
  test.setTimeout(240_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  // automated browsers keep the light circuit unless the preview is requested
  await page.goto("/?menupreview=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  await expect(page.locator("#mode-label")).toContainText("Pine Ridge");
  await expect(page.locator("#solo-track-choice")).toHaveValue("pine-ridge");
  await expect(page.locator("#solo-drive-link")).toHaveAttribute("href", /track=pine-ridge/);
  await expect(page.locator("#solo-entry-title")).toHaveText("Practice on Pine Ridge");
  await expect(page.locator("#network-status")).toHaveText("Connected to this server");
  // a test-circuit room: the preview gives way to it (new rooms default to the scenic track)
  await page.locator("#network-track-choice").selectOption("track-profile-project-circuit");
  await page.locator("#race-name").fill("Preview room");
  await page.getByRole("button", { name: "Create race" }).click();
  await expect(page.locator("#room-controls")).toBeVisible();
  await expect(page.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/, { timeout: 30_000 });
  await expect(page.locator("#mode-label")).not.toContainText("Pine Ridge");
  expect(errors).toEqual([]);
});

test("solo without a track choice lands on Pine Ridge; the circuit stays selectable; leaving a room restores the preview", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the live room automation runs once");
  test.setTimeout(240_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  // ?practice=local without a track: the scenic default (with the preview on) ...
  await page.goto("/?practice=local&ai=0&menupreview=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  await expect(page.locator("#solo-track-switch")).toHaveValue("pine-ridge");
  // ... the circuit is an explicit choice that sticks ...
  await page.goto("/?practice=local&ai=0&menupreview=1&track=test-circuit");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu/, { timeout: 90_000 });
  await expect(page.locator("#solo-track-switch")).toHaveValue("test-circuit");
  await expect(page.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/);
  // ... and automation keeps the circuit by default.
  await page.goto("/?practice=local&ai=0");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu/, { timeout: 90_000 });
  await expect(page.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/);
  // Lobby: a test-circuit room gives way to the circuit; leaving it brings the scenic preview back.
  await page.goto("/?menupreview=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  await expect(page.locator("#network-status")).toHaveText("Connected to this server");
  await page.locator("#network-track-choice").selectOption("track-profile-project-circuit");
  await page.locator("#race-name").fill("Leave room");
  await page.getByRole("button", { name: "Create race" }).click();
  await expect(page.locator("#room-controls")).toBeVisible();
  await expect(page.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/, { timeout: 30_000 });
  await page.locator("#leave-race").click();
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 60_000 });
  expect(errors).toEqual([]);
});

test("computer opponents race a generated track and complete a lap", async ({ page }) => {
  test.setTimeout(300_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?practice=local&track=night-harbour&ai=3&skill=1&catchup=1");
  await waitForTrack(page, "night-harbour");
  await expect(page.locator("#tick")).toHaveAttribute("data-opponent-count", "3", { timeout: 30_000 });
  const opponents = async () => JSON.parse((await page.locator("#tick").getAttribute("data-opponents")) ?? "[]");
  await expect.poll(async () => Math.max(...(await opponents()).map((car) => car.speed)), { timeout: 60_000 }).toBeGreaterThan(5);
  await expect.poll(async () => {
    const now = await opponents();
    return now.length === 3 && now.every((car) => car.laps >= 1);
  }, { timeout: 200_000, intervals: [1000] }).toBe(true);
  expect(errors).toEqual([]);
});

test("unknown track ids fall back to the test circuit without errors", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?practice=local&track=does-not-exist");
  await expect(page.locator("#engine-status")).toContainText(/Unknown track|wgpu/, { timeout: 60_000 });
  await expect.poll(async () => Number(await page.locator("#tick").textContent())).toBeGreaterThan(3);
  await expect(page.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/);
  expect(errors).toEqual([]);
});

/** Count pixels of the game area matching the named colour rule. */
async function countPaint(page, rule) {
  const png = await page.locator("#game").screenshot();
  return page.evaluate(async ({ data, rule }) => {
    const image = new Image();
    image.src = `data:image/png;base64,${data}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d");
    context.drawImage(image, 0, 0);
    const { data: px } = context.getImageData(0, 0, canvas.width, canvas.height);
    const rules = {
      blue: (r, g, b) => b > 140 && r < 90 && g < 130 && b - r > 70,
      orange: (r, g, b) => r > 170 && g > 70 && g < 160 && b < 90 && r - b > 100 && g - b > 30,
    };
    let count = 0;
    for (let i = 0; i < px.length; i += 4) if (rules[rule](px[i], px[i + 1], px[i + 2])) count += 1;
    return count;
  }, { data: png.toString("base64"), rule });
}

test("a generated track shows the chosen demo car and paint, other cars for the opponents", async ({ page }, testInfo) => {
  test.setTimeout(240_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/?practice=local&track=pine-ridge&ai=3&skill=1&catchup=1&car=quill-coupe&paint=harbour-blue");
  await waitForTrack(page, "pine-ridge");
  const tick = page.locator("#tick");
  await expect(tick).toHaveAttribute("data-project-car", "quill-coupe:harbour-blue");
  // the selector is usable on the generated track and the install covers the whole field
  await expect(page.locator("#project-car-setup")).toBeVisible();
  const field = (await tick.getAttribute("data-project-field")).split(",");
  expect(field).toHaveLength(4);
  expect(field[0]).toBe("pc-quill-coupe-harbour-blue");
  expect(new Set(field).size).toBe(4);
  // the frames draw the player's car first and three other models
  await expect(tick).toHaveAttribute("data-project-render", /^quill-coupe:harbour-blue,/, { timeout: 30_000 });
  const drawn = (await tick.getAttribute("data-project-render")).split(",");
  expect(drawn).toHaveLength(4);
  expect(new Set(drawn.map((entry) => entry.split(":")[0])).size).toBe(4);
  // chosen paint on screen: blue car, then orange after the switch
  await expect.poll(() => countPaint(page, "blue"), { timeout: 30_000 }).toBeGreaterThan(150);
  const orangeBefore = await countPaint(page, "orange");
  await page.locator("#project-paint-choice").selectOption("ember-orange");
  await expect(tick).toHaveAttribute("data-project-car", "quill-coupe:ember-orange");
  await expect(tick).toHaveAttribute("data-project-render", /^quill-coupe:ember-orange,/, { timeout: 30_000 });
  await expect.poll(() => countPaint(page, "orange"), { timeout: 30_000 }).toBeGreaterThan(orangeBefore + 150);
  await page.screenshot({ path: testInfo.outputPath("generated-track-car.png"), scale: "css", clip: { x: 0, y: 0, width: 640, height: 400 } });
  // a different model replaces the player's car
  await page.locator("#project-car-choice").selectOption("shard-gt");
  await expect(tick).toHaveAttribute("data-project-render", /^shard-gt:/, { timeout: 30_000 });
  expect(errors).toEqual([]);
});

for (const id of TRACKS) {
  test(`generated track ${id} renders from the chase camera and measures fps`, async ({ page }, testInfo) => {
    test.setTimeout(300_000);
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const record = { track: id, browser: testInfo.project.name, spots: {} };
    mkdirSync(shotDir, { recursive: true });
    for (const spot of SPOTS) {
      const trackId = spot === 0 ? id : `${id}-s${String(spot).padStart(2, "0")}`;
      await page.goto(`/?practice=local&track=${trackId}`);
      await waitForTrack(page, trackId);
      await fixCanvasSize(page);
      // let the chase camera settle, then measure the frame rate for a few seconds
      await page.waitForTimeout(1500);
      const fps = await page.evaluate(() => new Promise((resolve) => {
        const seconds = 3;
        const tickEl = document.querySelector("#tick");
        const ticks0 = Number(tickEl.textContent);
        let frames = 0;
        const t0 = performance.now();
        const loop = () => {
          frames++;
          const elapsed = (performance.now() - t0) / 1000;
          if (elapsed < seconds) requestAnimationFrame(loop);
          else resolve({ fps: frames / elapsed, simHz: (Number(tickEl.textContent) - ticks0) / elapsed });
        };
        requestAnimationFrame(loop);
      }));
      record.spots[spot] = { fps: Number(fps.fps.toFixed(1)), simHz: Number(fps.simHz.toFixed(1)) };
      const canvas = page.locator("canvas.render-surface:visible");
      const name = `${id}-${String(spot).padStart(2, "0")}.png`;
      const image = await canvas.screenshot({ path: testInfo.outputPath(name) });
      expect(image.length).toBeGreaterThan(5000);
      if (testInfo.project.name === "chromium") writeFileSync(join(shotDir, name), image);
      expect((await canvas.boundingBox()).width).toBe(640);
    }
    // sanity: the rendered frame is not blank or uniform
    const frame = await page.locator("canvas.render-surface:visible").screenshot();
    expect(new Set(frame.subarray(0, 4000)).size).toBeGreaterThan(20);
    if (testInfo.project.name === "chromium") writeFileSync(join(shotDir, `${id}.json`), JSON.stringify(record, null, 2));
    console.info(`[${testInfo.project.name}] ${id} fps ${JSON.stringify(record.spots)}`);
    expect(errors).toEqual([]);
  });
}

test("two browsers race on a generated track through the lobby", async ({ browser, browserName }, testInfo) => {
  test.skip(browserName !== "chromium", "the live two-browser race runs once");
  test.setTimeout(300_000);
  const hostContext = await browser.newContext();
  const guestContext = await browser.newContext();
  const host = await hostContext.newPage();
  const guest = await guestContext.newPage();
  const errors = [];
  for (const page of [host, guest]) page.on("pageerror", (error) => errors.push(error.message));
  await Promise.all([host.goto("/?car=shard-gt&paint=ember-orange"), guest.goto("/?car=tumble-rally&paint=plum")]);
  await expect(host.locator("#network-status")).toHaveText("Connected to this server", { timeout: 60_000 });
  await expect(guest.locator("#network-status")).toHaveText("Connected to this server", { timeout: 60_000 });
  const choice = host.locator("#network-track-choice");
  await expect(choice.locator("option")).toHaveCount(5, { timeout: 60_000 });
  await choice.selectOption("track-profile-rcp-red-mesa");
  await expect(host.locator("#race-laps")).toBeEnabled();
  await host.locator("#race-name").fill("Generated mesa race");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("0");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible({ timeout: 60_000 });
  await expect(host.locator("#tick")).toHaveAttribute("data-project-track", "red-mesa", { timeout: 60_000 });
  const guestRace = guest.locator(".race-row", { hasText: "Generated mesa race" });
  await expect(guestRace).toBeVisible();
  await guestRace.getByRole("button", { name: "Join" }).click();
  await expect(guest.locator("#room-controls")).toBeVisible({ timeout: 60_000 });
  await expect(guest.locator("#tick")).toHaveAttribute("data-project-track", "red-mesa", { timeout: 60_000 });
  await host.getByRole("button", { name: "Ready" }).click();
  await guest.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(guest.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(host.locator("#tick")).toHaveAttribute("data-vehicle-count", "2");
  await expectLap(host, 1, 1);
  const startX = Number(await host.locator("#tick").getAttribute("data-x"));
  await host.locator("canvas.render-surface:visible").click();
  await host.keyboard.down("w");
  await expect.poll(async () => Number(await host.locator("#tick").getAttribute("data-x")), { timeout: 60_000 }).toBeGreaterThan(startX + 40);
  await expect(guest.locator("#tick")).toHaveAttribute("data-remote-vehicle-count", "1");
  await host.keyboard.up("w");
  // everybody sees the cars the players picked (the protocol carries model and paint)
  await expect(guest.locator("#tick")).toHaveAttribute("data-project-render", /shard-gt:ember-orange/, { timeout: 30_000 });
  await expect(guest.locator("#tick")).toHaveAttribute("data-project-render", /^tumble-rally:plum|,tumble-rally:plum/, { timeout: 30_000 });
  await expect(host.locator("#tick")).toHaveAttribute("data-project-render", /tumble-rally:plum/, { timeout: 30_000 });
  await expect(host.locator("#tick")).toHaveAttribute("data-project-render", /^shard-gt:ember-orange|,shard-gt:ember-orange/, { timeout: 30_000 });
  // the guest sees the room track and the other car rendered (not a blank frame)
  const frame = await guest.locator("canvas.render-surface:visible").screenshot({ path: testInfo.outputPath("guest-room.png") });
  expect(new Set(frame.subarray(0, 4000)).size).toBeGreaterThan(20);
  await host.getByRole("button", { name: "Leave race" }).click();
  await expect(host.locator("#room-controls")).toBeHidden({ timeout: 10_000 });
  await expect(host.locator("#tick")).not.toHaveAttribute("data-project-track", /.+/, { timeout: 30_000 });
  expect(errors).toEqual([]);
  await hostContext.close();
  await guestContext.close();
});
