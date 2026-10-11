import { expect, test } from "@playwright/test";

// Computer opponents on the project test circuit. The page runs the fixed-step
// simulation locally; the drivers are the generic engine AI (crates/ai).
const opponents = async (page) => JSON.parse(await page.locator("#tick").getAttribute("data-opponents") ?? "[]");

test("solo race on the test circuit with three computer opponents: they move, are ranked and complete laps", async ({ page }) => {
  test.setTimeout(240_000);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 960, height: 600 });
  await page.goto("/?practice=local&ai=3&skill=1&catchup=1");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#hud-state")).toHaveText("PRACTICE");
  await expect(page.locator("#tick")).toHaveAttribute("data-opponent-count", "3");
  // The player starts on the rearmost grid slot of four cars.
  await expect(page.locator("#hud-position")).toHaveText("P4/4");
  // The canvas HUD (built-in unless a plugin replaces it) carries the same position, and the built-in mini map tracks the three rivals.
  await expect(page.locator("#hud-canvas")).toHaveAttribute("data-position", "4/4");
  if (await page.evaluate(() => window.RacerPlugins.first("hud")?.builtin === true)) {
    await expect(page.locator("#hud-canvas")).toHaveAttribute("data-rivals", "3");
  }
  const start = await opponents(page);
  expect(start).toHaveLength(3);

  // Without any input from the player the opponents leave the grid ...
  await expect.poll(async () => {
    const now = await opponents(page);
    return now.length === 3 && now.every((car, i) => Math.hypot(car.x - start[i].x, car.y - start[i].y) > 5);
  }, { timeout: 60_000 }).toBe(true);
  await expect.poll(async () => Math.max(...(await opponents(page)).map((car) => car.speed))).toBeGreaterThan(3);

  // ... and complete laps of the circuit.
  await expect.poll(async () => {
    const now = await opponents(page);
    return now.every((car) => car.laps >= 1);
  }, { timeout: 150_000, intervals: [1000] }).toBe(true);
  const ranks = (await opponents(page)).map((car) => car.position);
  expect(ranks.every((rank) => rank >= 1 && rank <= 3)).toBe(true);
  // The parked player is behind the opponents that have lapped.
  await expect(page.locator("#hud-position")).toHaveText("P4/4");
  expect(pageErrors).toEqual([]);
});

test("solo set-up offers opponents, difficulty and catch-up and carries them into the session", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("#solo-ai-count")).toHaveValue("7");
  await expect(page.locator("#solo-ai-skill")).toHaveValue("1");
  await expect(page.locator("#solo-ai-catchup")).toBeChecked();
  await page.locator("#solo-ai-count").selectOption("2");
  await page.locator("#solo-ai-skill").selectOption("0");
  await page.locator("#solo-ai-catchup").uncheck();
  await expect(page.locator("#solo-drive-link")).toHaveAttribute("href", "?practice=local&ai=2&skill=0&catchup=0");
  await page.locator("#solo-drive-link").click();
  await expect(page.locator("#practice-ai-count")).toHaveValue("2");
  await expect(page.locator("#practice-ai-skill")).toHaveValue("0");
  await expect(page.locator("#practice-ai-catchup")).not.toBeChecked();
  await expect(page.locator("#practice-ai-note")).toContainText("2 computer opponents · Beginner · catch-up off");
  await expect(page.locator("#tick")).toHaveAttribute("data-opponent-count", "2");
});

test("room set-up creates a race with computer opponents that start with a single human", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once");
  test.setTimeout(120_000);
  await page.goto("/");
  await expect(page.locator("#network-status")).toHaveText("Connected to this server");
  await expect(page.locator("#race-ai-count")).toHaveValue("7");
  await page.locator("#race-name").fill("CPU room");
  await page.locator("#race-laps").selectOption("1");
  await page.locator("#race-ai-count").selectOption("2");
  await page.getByRole("button", { name: "Create race" }).click();
  await expect(page.locator("#room-controls")).toBeVisible();
  await expect(page.locator("#room-ai-wrap")).toBeVisible();
  await expect(page.locator("#room-ai-count")).toHaveValue("2");
  await page.locator("#room-ai-count").selectOption("3");
  await expect(page.locator(".race-row", { hasText: "CPU room" })).toContainText("+ 3 CPU");
  await page.getByRole("button", { name: "Ready" }).click();
  await expect(page.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(page.locator("#tick")).toHaveAttribute("data-opponent-count", "3");
  await expect(page.locator("#hud-position")).toHaveText(/^P\d\/4$/);
  const start = await opponents(page);
  await expect.poll(async () => {
    const now = await opponents(page);
    return now.length === 3 && now.every((car) => car.speed > 1);
  }, { timeout: 30_000 }).toBe(true);
  expect(start).toHaveLength(3);
});

test("pressing R after driving ahead updates the race position and gate target at once", async ({ page }) => {
  await page.goto("/?practice=local&ai=3&skill=1&catchup=1");
  await expect(page.locator("#hud-position")).toHaveText("P4/4");
  await page.locator("#race-canvas").focus();
  await page.keyboard.down("ArrowUp");
  await expect.poll(async () => Number(await page.locator("#tick").getAttribute("data-speed")), { timeout: 20_000 }).toBeGreaterThan(5);
  await page.keyboard.up("ArrowUp");
  await page.keyboard.press("r");
  await expect(page.locator("#tick")).toHaveAttribute("data-checkpoint", "1", { timeout: 1_000 });
  await expect(page.locator("#hud-position")).toHaveText("P4/4", { timeout: 1_000 });
});
