import { expect, test } from "@playwright/test";

// Reverse gear end to end (tests/feel and
// tests/feel): the automatic box starts in first gear,
// shift-down (Z) twice selects R (via N), accelerate then drives backward, and
// shift-up (A) twice returns to forward driving. The manual box starts in N
// and A/Z walk the gears one step per tap. Observed through #tick data
// attributes: data-gear-code (250 R, 251 N, 1..n forward) and the signed
// data-forward-speed (negative while reversing).

// Two live WebGL pages: tracing makes each action several times slower on software GL.
test.use({ trace: "off" });

const GEAR_R = "250";
const GEAR_N = "251";

const forwardSpeed = (page) => page.locator("#tick").evaluate((el) => Number(el.dataset.forwardSpeed));

async function tap(page, code, ms = 70) {
  await page.keyboard.down(code);
  await page.waitForTimeout(ms);
  await page.keyboard.up(code);
  await page.waitForTimeout(90);
}

/** Press and release inside one task: shorter than any frame or network sample. */
async function burstTap(page, code) {
  await page.evaluate((c) => {
    window.dispatchEvent(new KeyboardEvent("keydown", { code: c, bubbles: true }));
    window.dispatchEvent(new KeyboardEvent("keyup", { code: c, bubbles: true }));
  }, code);
  await page.waitForTimeout(150);
}

test("solo on a generated track: Z Z, accelerate reverses; A A returns to forward", async ({ page }) => {
  test.setTimeout(120_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  const tick = page.locator("#tick");
  await expect(tick).toHaveAttribute("data-gear-code", "1");
  await expect(page.locator("#controls-hint")).toContainText("Z Z at standstill: reverse");

  await tap(page, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N);
  await tap(page, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);
  await tap(page, "KeyZ"); // further presses change nothing in R
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);

  await page.keyboard.down("ArrowUp");
  await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeLessThan(-3);
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);
  await page.keyboard.up("ArrowUp");

  // Stop, then A A: R -> N -> 1 and the car drives forward.
  await page.keyboard.down("ArrowDown");
  await expect.poll(async () => Math.abs(await forwardSpeed(page)), { timeout: 20_000 }).toBeLessThan(0.3);
  await page.keyboard.up("ArrowDown");
  await tap(page, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N);
  await tap(page, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", /^[1-9]$/);
  await page.keyboard.down("ArrowUp");
  await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeGreaterThan(3);
  await page.keyboard.up("ArrowUp");
  expect(errors).toEqual([]);
});

test("solo: a key press shorter than a frame still shifts, and the neutral hint appears", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  const tick = page.locator("#tick");
  await burstTap(page, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N);
  await burstTap(page, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);
  await burstTap(page, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N);
  // Accelerate in N: nothing moves and after a second the hint says what to do.
  await expect(page.locator("#gear-hint")).toBeHidden();
  await page.keyboard.down("ArrowUp");
  await expect(page.locator("#gear-hint")).toHaveText("Press A to select 1st", { timeout: 10_000 });
  expect(Math.abs(await forwardSpeed(page))).toBeLessThan(0.5);
  await page.keyboard.up("ArrowUp");
  await burstTap(page, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", /^[1-9]$/);
  await expect(page.locator("#gear-hint")).toBeHidden();
});

test("solo: auto reverse is off by default and brake alone then never reverses", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  await expect(page.locator("#auto-reverse")).not.toBeChecked();
  await page.keyboard.down("ArrowDown");
  await page.waitForTimeout(2500);
  await expect(page.locator("#tick")).toHaveAttribute("data-gear-code", "1");
  await page.keyboard.up("ArrowDown");
});

for (const gearbox of ["automatic", "manual"]) {
  test(`solo ${gearbox}: auto reverse needs no shifting (brake reverses, throttle brakes then drives on)`, async ({ page }) => {
    test.setTimeout(180_000);
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const gearParam = gearbox === "manual" ? "&gearbox=manual" : "";
    await page.goto(`/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1&autoreverse=1${gearParam}`);
    const tick = page.locator("#tick");
    await expect(tick).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
    await expect(page.locator("#auto-reverse")).toBeChecked();
    await expect(page.locator("#controls-hint")).toContainText("reverse");
    await expect(page.locator("#controls-hint")).not.toContainText("Z Z");

    // Drive forward first (the manual box leaves N by itself under auto reverse).
    await page.keyboard.down("ArrowUp");
    await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeGreaterThan(5);
    await page.keyboard.up("ArrowUp");

    // Brake while rolling forward only brakes (the sim tests pin the threshold); holding on:
    // stopped, then R, then backwards.
    await page.keyboard.down("ArrowDown");
    await expect(tick).toHaveAttribute("data-gear-code", GEAR_R, { timeout: 20_000 });
    await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeLessThan(-3);
    await page.keyboard.up("ArrowDown");

    // Throttle in R brakes first, then 1st gear and forward.
    await page.keyboard.down("ArrowUp");
    await expect(tick).toHaveAttribute("data-gear-code", /^[1-9]$/, { timeout: 20_000 });
    await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeGreaterThan(3);
    await page.keyboard.up("ArrowUp");
    expect(errors).toEqual([]);
  });
}

test("solo: ticking the box at run time turns auto reverse on", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1");
  const tick = page.locator("#tick");
  await expect(tick).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  await page.locator("#auto-reverse").check();
  await page.keyboard.down("ArrowDown");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R, { timeout: 10_000 });
  await page.keyboard.up("ArrowDown");
});

test("room with two browsers: Z Z then accelerate drives the host backward on the server", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once");
  test.setTimeout(240_000);
  const host = await (await browser.newContext()).newPage();
  const guest = await (await browser.newContext()).newPage();
  await Promise.all([host.goto("/?hud=diag"), guest.goto("/?hud=diag")]);
  await expect(host.locator("#network-status")).toHaveText("Connected to this server");
  await expect(guest.locator("#network-status")).toHaveText("Connected to this server");

  await host.locator("#race-name").fill("Reverse pair");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("0");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  await guest.locator(".race-row", { hasText: "Reverse pair" }).getByRole("button", { name: "Join" }).click();
  await expect(guest.locator("#room-controls")).toBeVisible();
  await host.getByRole("button", { name: "Ready" }).click();
  await guest.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(guest.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });

  const tick = host.locator("#tick");
  await expect(tick).toHaveAttribute("data-gear-code", "1", { timeout: 20_000 });
  await burstTap(host, "KeyZ"); // shorter than a network sample: must still arrive
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N, { timeout: 10_000 });
  await burstTap(host, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R, { timeout: 10_000 });
  await tap(host, "KeyZ");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);
  await host.keyboard.down("ArrowUp");
  await expect.poll(() => forwardSpeed(host), { timeout: 20_000 }).toBeLessThan(-3);
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R);
  await host.keyboard.up("ArrowUp");
  await expect(guest.locator("#tick")).toHaveAttribute("data-vehicle-count", "2");
});

test("solo manual gearbox: starts in N, A/Z walk the gears, speed only grows after shifting up", async ({ page }) => {
  test.setTimeout(180_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?practice=local&track=pine-ridge&ai=0&skill=1&catchup=1&gearbox=manual");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-track", "pine-ridge", { timeout: 90_000 });
  const tick = page.locator("#tick");
  await expect(page.locator("#practice-transmission")).toHaveValue("manual");
  await expect(page.locator("#controls-hint")).toContainText("Manual gearbox");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N);

  // Throttle in neutral only revs.
  await page.keyboard.down("ArrowUp");
  await page.waitForTimeout(1500);
  expect(Math.abs(await forwardSpeed(page))).toBeLessThan(0.5);
  await page.keyboard.up("ArrowUp");

  // One step per tap: N -> 1 -> 2 -> 1 -> N -> R -> R, then back up to N.
  const walk = [["KeyA", "1"], ["KeyA", "2"], ["KeyZ", "1"], ["KeyZ", GEAR_N], ["KeyZ", GEAR_R], ["KeyZ", GEAR_R], ["KeyA", GEAR_N], ["KeyA", "1"]];
  for (const [key, gear] of walk) {
    await tap(page, key);
    await expect(tick).toHaveAttribute("data-gear-code", gear);
  }

  // Held in first gear the speed locks at the rev limit ...
  await page.keyboard.down("ArrowUp");
  await page.waitForTimeout(5000);
  const first = await forwardSpeed(page);
  expect(first).toBeGreaterThan(8);
  await page.waitForTimeout(1500);
  expect(Math.abs((await forwardSpeed(page)) - first)).toBeLessThan(0.6);
  // ... and only a shift up lets the car go faster.
  await tap(page, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", "2");
  await expect.poll(() => forwardSpeed(page), { timeout: 20_000 }).toBeGreaterThan(first + 3);
  await page.keyboard.up("ArrowUp");
  expect(errors).toEqual([]);
});

test("solo: the gearbox selector in the entry panel writes the link and the practice selector switches at once", async ({ page }) => {
  await page.goto("/");
  await page.locator("#solo-transmission").selectOption("manual");
  await expect(page.locator("#solo-drive-link")).toHaveAttribute("href", /gearbox=manual/);
  await page.locator("#solo-transmission").selectOption("automatic");
  await expect(page.locator("#solo-drive-link")).not.toHaveAttribute("href", /gearbox/);
});

test("room: the lobby gearbox selector gives the host a manual box on the server", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once");
  test.setTimeout(240_000);
  const host = await (await browser.newContext()).newPage();
  await host.goto("/?hud=diag");
  await expect(host.locator("#network-status")).toHaveText("Connected to this server");
  await host.locator("#race-name").fill("Manual room");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("1");
  await host.locator("#race-transmission").selectOption("manual");
  await expect(host.locator("#controls-hint")).toContainText("Manual gearbox");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  await expect(host.locator("#room-transmission")).toHaveValue("manual");
  await host.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  const tick = host.locator("#tick");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N, { timeout: 20_000 });
  await host.keyboard.down("ArrowUp");
  await host.waitForTimeout(1500);
  expect(Math.abs(await forwardSpeed(host))).toBeLessThan(0.5);
  await host.keyboard.up("ArrowUp");
  await tap(host, "KeyA");
  await expect(tick).toHaveAttribute("data-gear-code", "1", { timeout: 10_000 });
  await host.keyboard.down("ArrowUp");
  await expect.poll(() => forwardSpeed(host), { timeout: 20_000 }).toBeGreaterThan(3);
  await host.keyboard.up("ArrowUp");
});

test("room: auto reverse is applied by the server (no shifting, manual lobby box)", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once");
  test.setTimeout(240_000);
  const host = await (await browser.newContext()).newPage();
  await host.goto("/?hud=diag&autoreverse=1");
  await expect(host.locator("#network-status")).toHaveText("Connected to this server");
  await expect(host.locator("#auto-reverse")).toBeChecked();
  await host.locator("#race-name").fill("Auto reverse room");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("1");
  await host.locator("#race-transmission").selectOption("manual");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  await host.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  const tick = host.locator("#tick");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_N, { timeout: 20_000 });
  // Brake from the standing start reverses without any shift.
  await host.keyboard.down("ArrowDown");
  await expect(tick).toHaveAttribute("data-gear-code", GEAR_R, { timeout: 10_000 });
  await expect.poll(() => forwardSpeed(host), { timeout: 20_000 }).toBeLessThan(-3);
  await host.keyboard.up("ArrowDown");
  // Throttle in R brakes, then selects first gear and drives forward.
  await host.keyboard.down("ArrowUp");
  await expect(tick).toHaveAttribute("data-gear-code", /^[1-9]$/, { timeout: 20_000 });
  await expect.poll(() => forwardSpeed(host), { timeout: 20_000 }).toBeGreaterThan(3);
  await host.keyboard.up("ArrowUp");
});
