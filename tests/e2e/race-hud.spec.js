import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

test("compact classic-layout race HUD shows position, lap and timer while driving, and the Sound mute toggles cleanly", async ({ page }, testInfo) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 960, height: 600 });
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#hud-state")).toHaveText("PRACTICE");

  // The canvas HUD is the built-in arcade HUD unless a plugin provides its own (-WithPlugin).
  // Values are exposed as data attributes on the HUD canvas; the old cards are hidden by default.
  const arcade = await page.evaluate(() => window.RacerPlugins.first("hud")?.builtin === true);
  const hud = page.locator("#hud-canvas");
  await expect(hud).toBeVisible();
  await expect(hud).toHaveAttribute("data-art", arcade ? "arcade" : "project");
  await expect(page.locator("#game-hud")).toBeHidden();
  await expect(page.locator("#hud-race")).toBeHidden();
  await expect(hud).toHaveAttribute("data-position", "1/1");
  await expect(hud).toHaveAttribute("data-lap", "1/2");
  await expect(hud).toHaveAttribute("data-record", "-:--.--");
  // the built-in HUD flashes GO for the first second; wait it out
  await expect(hud).toHaveAttribute("data-banner", "normal");

  const paintedFractions = () => hud.evaluate((element, arcade) => {
    const context = element.getContext("2d");
    const { width, height } = element;
    const data = context.getImageData(0, 0, width, height).data;
    const share = (x0, y0, x1, y1) => {
      let painted = 0, total = 0;
      for (let y = Math.floor(y0 * height); y < Math.floor(y1 * height); y++) {
        for (let x = Math.floor(x0 * width); x < Math.floor(x1 * width); x++) {
          total++;
          if (data[(y * width + x) * 4 + 3] > 0) painted++;
        }
      }
      return painted / total;
    };
    return arcade ? {
      gauge: share(0.02, 0.03, 0.24, 0.22),
      banner: share(0.35, 0.03, 0.65, 0.08),
      map: share(0.8, 0.8, 0.98, 0.98),
      middle: share(0.3, 0.3, 0.7, 0.7),
    } : {
      gauge: share(0.02, 0.02, 0.24, 0.13),
      banner: share(0.4, 0.01, 0.98, 0.07),
      middle: share(0.3, 0.3, 0.7, 0.7),
    };
  }, arcade);
  await expect.poll(async () => (await paintedFractions()).gauge).toBeGreaterThan(0.05);
  const painted = await paintedFractions();
  expect(painted.banner).toBeGreaterThan(0.05);
  if (arcade) expect(painted.map).toBeGreaterThan(0.2); // mini map panel of the test circuit
  expect(painted.middle).toBeLessThan(0.02); // the 3D view stays uncovered

  const canvas = page.locator("canvas.render-surface:visible");
  await canvas.click();
  await page.keyboard.down("w");
  await expect.poll(async () => Number(await page.locator("#speed").textContent())).toBeGreaterThan(0);
  await expect.poll(async () => Number(await hud.getAttribute("data-speed"))).toBeGreaterThan(0);
  const first = await hud.getAttribute("data-timer");
  await expect.poll(async () => hud.getAttribute("data-timer")).not.toBe(first);
  await expect(hud).toHaveAttribute("data-position", "1/1");
  await expect(hud).toHaveAttribute("data-lap", "1/2");

  // Headless 640 px screenshot of the driving view, kept under test-results (Chromium only).
  if (testInfo.project.name === "chromium") {
    await page.setViewportSize({ width: 640, height: 480 });
    await expect.poll(async () => hud.evaluate((element) => element.width)).toBeLessThanOrEqual(640);
    const out = testInfo.outputPath("hud-640.png");
    mkdirSync(dirname(out), { recursive: true });
    await page.screenshot({ path: out });
    await testInfo.attach("hud-640", { path: out, contentType: "image/png" });
  }
  await page.keyboard.up("w");

  // The card HUD stays available as diagnostics.
  await page.locator("#hud-diagnostics").scrollIntoViewIfNeeded();
  await page.locator("#hud-diagnostics").check();
  await expect(page.locator("#game-hud")).toBeVisible();
  await expect(page.locator("#hud-position")).toHaveText("P1/1");
  if (arcade) await expect(hud).toBeHidden();
  await page.locator("#hud-diagnostics").uncheck();
  await expect(page.locator("#game-hud")).toBeHidden();
  if (arcade) await expect(hud).toBeVisible();

  // Sound control exists and the mute checkbox toggles without errors.
  const volume = page.locator("#sound-volume");
  const mute = page.locator("#sound-muted");
  await expect(volume).toBeAttached();
  await expect(mute).toBeAttached();
  await mute.scrollIntoViewIfNeeded();
  const initiallyMuted = await mute.isChecked();
  await mute.setChecked(!initiallyMuted);
  await expect(mute).toBeChecked({ checked: !initiallyMuted });
  await mute.setChecked(initiallyMuted);
  await expect(mute).toBeChecked({ checked: initiallyMuted });
  await volume.fill("0.3");
  expect(pageErrors).toEqual([]);
});

test("?hud=diag shows the diagnostic card HUD instead of the built-in canvas HUD", async ({ page }) => {
  await page.goto("/?practice=local&hud=diag");
  await expect(page.locator("#hud-state")).toHaveText("PRACTICE");
  await expect(page.locator("#game-hud")).toBeVisible();
  await expect(page.locator("#hud-diagnostics")).toBeChecked();
  const providers = await page.evaluate(() => window.RacerPlugins.providers("hud").map((entry) => entry.pluginId));
  expect(providers).not.toContain("owr-arcade-hud");
  if (providers.length === 0) await expect(page.locator("#hud-canvas")).toBeHidden();
});
