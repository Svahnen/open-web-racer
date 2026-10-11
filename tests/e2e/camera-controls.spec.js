import { expect, test } from "@playwright/test";

test("mirror toggle and speed display preference reach the browser renderer and HUD", async ({ page }) => {
  await page.goto("/?practice=local&speedUnit=invalid");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
  await expect(page.locator("#tick")).toHaveAttribute("data-mirror-visible", "true");
  await expect(page.locator("#speed-display-unit")).toHaveValue("kmh");
  await expect(page.locator("#speed-unit")).toHaveText("km/h");

  await page.keyboard.press("F7");
  await expect(page.locator("#tick")).toHaveAttribute("data-mirror-visible", "false");
  await page.keyboard.down("F7");
  await expect(page.locator("#tick")).toHaveAttribute("data-mirror-visible", "true");
  await page.keyboard.down("F7");
  await expect(page.locator("#tick")).toHaveAttribute("data-mirror-visible", "true");
  await page.keyboard.up("F7");

  await page.locator("#speed-display-unit").selectOption("mph");
  await expect(page.locator("#speed-unit")).toHaveText("mph");
  await expect(page).toHaveURL(/speedUnit=mph/);
  await page.locator("#speed-display-unit").focus();
  await page.keyboard.press("F7");
  await expect(page.locator("#tick")).toHaveAttribute("data-mirror-visible", "true");
});

test("connected camera selector, C cycling, B look-behind, and release handling", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto("/");
  await expect(page.locator("#network-status")).toHaveText("Connected to this server");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);

  const camera = page.locator("#preview-camera-mode");
  const canvas = page.locator("canvas.render-surface:visible");
  await expect(camera).toBeVisible();
  await expect(camera).toBeEnabled();
  await expect(camera).toHaveValue("chase");
  await canvas.click();

  await page.keyboard.press("Control+c");
  await expect(camera).toHaveValue("chase");
  await page.locator("#race-name").fill("Editable field");
  await page.locator("#race-name").focus();
  await page.keyboard.press("c");
  await expect(page.locator("#race-name")).toHaveValue("Editable fieldc");
  await expect(camera).toHaveValue("chase");

  await canvas.click();
  const chaseFrame = await canvas.screenshot();
  await page.keyboard.press("c");
  await expect(camera).toHaveValue("wide");
  await expect.poll(async () => (await canvas.screenshot()).equals(chaseFrame), { timeout: 30_000 }).toBe(false);
  const wideFrame = await canvas.screenshot();
  for (const mode of ["close", "road", "chase"]) {
    await page.keyboard.press("c");
    await expect(camera).toHaveValue(mode);
  }
  await page.keyboard.press("c");
  await expect(camera).toHaveValue("wide");

  await page.keyboard.down("b");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "true");
  await expect.poll(async () => (await canvas.screenshot()).equals(wideFrame), { timeout: 30_000 }).toBe(false);
  await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "false");
  await page.keyboard.up("b");

  for (const width of [320, 360, 640, 900]) {
    await page.setViewportSize({ width, height: 720 });
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    const bounds = await camera.boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
  }
  await page.setViewportSize({ width: 1024, height: 768 });

  await page.locator("#race-name").fill("Camera release room");
  await page.getByRole("button", { name: "Create race" }).click();
  await expect(page.locator("#room-controls")).toBeVisible();
  await page.keyboard.down("b");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "true");
  await page.evaluate(() => document.querySelector("#leave-race").click());
  await expect(page.locator("#room-controls")).toBeHidden();
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "false");
  await page.keyboard.up("b");
  expect(pageErrors).toEqual([]);
});

test("connected camera input changes rendering while authoritative motion stays unchanged", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the two-client camera authority check runs once");
  test.setTimeout(180_000);
  const hostContext = await browser.newContext();
  const guestContext = await browser.newContext();
  const host = await hostContext.newPage();
  const guest = await guestContext.newPage();
  await Promise.all([host.goto("/"), guest.goto("/")]);
  await expect(host.locator("#network-status")).toHaveText("Connected to this server");
  await expect(guest.locator("#network-status")).toHaveText("Connected to this server");
  await host.locator("#race-name").fill("Camera authority check");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  const guestRace = guest.locator(".race-row", { hasText: "Camera authority check" });
  await expect(guestRace).toBeVisible();
  await guestRace.getByRole("button", { name: "Join" }).click();
  await expect(guest.locator("#room-controls")).toBeVisible();
  await host.getByRole("button", { name: "Ready" }).click();
  await guest.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(guest.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });

  const tick = host.locator("#tick");
  await expect(tick).toHaveAttribute("data-server", "true");
  const before = {
    heading: await tick.getAttribute("data-heading"),
    speed: await tick.getAttribute("data-speed"),
  };
  await host.keyboard.press("c");
  await expect(host.locator("#preview-camera-mode")).toHaveValue("wide");
  await host.keyboard.down("b");
  await expect(tick).toHaveAttribute("data-look-behind", "true");
  await host.waitForTimeout(250);
  await expect(tick).toHaveAttribute("data-heading", before.heading);
  await expect(tick).toHaveAttribute("data-speed", before.speed);
  await expect(host.locator("#speed")).toHaveText("0.0");
  await host.keyboard.up("b");
  await expect(tick).toHaveAttribute("data-look-behind", "false");
  await host.close();
  await guest.close();
});
