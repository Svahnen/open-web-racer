import { expect, test } from "@playwright/test";

test("fullscreen contains only the game view, resizes, and releases driving input on exit", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?hud=diag");
  await expect(page.locator("#network-status")).toHaveText("Connected to this server");

  const game = page.locator("#game");
  const toggle = page.locator("#fullscreen-toggle");
  const canvas = page.locator(".game canvas.render-surface:visible").first();
  await expect(toggle).toBeEnabled();
  const pageBounds = await game.boundingBox();
  await toggle.click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
  await expect(toggle).toHaveText("Exit fullscreen");
  await expect(toggle).toHaveAttribute("aria-pressed", "true");

  const fullscreenBounds = await game.boundingBox();
  const viewport = await page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
  expect(fullscreenBounds.x).toBe(0);
  expect(fullscreenBounds.y).toBe(0);
  expect(Math.abs(fullscreenBounds.width - viewport.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(fullscreenBounds.height - viewport.height)).toBeLessThanOrEqual(1);
  const canvasBounds = await canvas.boundingBox();
  expect(Math.abs(canvasBounds.width - fullscreenBounds.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(canvasBounds.height - fullscreenBounds.height)).toBeLessThanOrEqual(1);
  const hud = page.locator("#game-hud");
  await expect(hud).toBeVisible();
  const hudBounds = await hud.boundingBox();
  expect(hudBounds.x).toBeGreaterThanOrEqual(fullscreenBounds.x);
  expect(hudBounds.y).toBeGreaterThanOrEqual(fullscreenBounds.y);
  expect(hudBounds.x + hudBounds.width).toBeLessThanOrEqual(fullscreenBounds.width + 1);
  const mapPanel = page.locator("#route-map-panel");
  if (await mapPanel.isVisible()) {
    const mapBounds = await mapPanel.boundingBox();
    expect(mapBounds.x).toBeGreaterThanOrEqual(fullscreenBounds.x);
    expect(mapBounds.y).toBeGreaterThanOrEqual(fullscreenBounds.y);
    expect(mapBounds.x + mapBounds.width).toBeLessThanOrEqual(fullscreenBounds.width + 1);
    expect(mapBounds.y + mapBounds.height).toBeLessThanOrEqual(fullscreenBounds.height + 1);
  }
  const lobbyIsOutsideFullscreen = await page.evaluate(() => {
    const fullscreen = document.fullscreenElement;
    return !!fullscreen
      && !fullscreen.contains(document.querySelector(".network-panel"))
      && !fullscreen.contains(document.querySelector("#create-race-form"));
  });
  expect(lobbyIsOutsideFullscreen).toBe(true);

  await page.keyboard.down("b");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "true");
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("");
  await expect(toggle).toHaveText("Enter fullscreen");
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "false");
  const canvasId = await canvas.getAttribute("id");
  await expect.poll(() => page.evaluate(() => document.activeElement?.id ?? "")).toBe(canvasId);
  await expect.poll(async () => (await game.boundingBox()).width).toBe(pageBounds.width);
  await page.keyboard.up("b");
  expect(errors).toEqual([]);
});

test("fullscreen backing buffer follows viewport and device pixel ratio", async ({ page }) => {
  await page.goto("/?hud=diag");
  await page.locator("#fullscreen-toggle").click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
  await page.setViewportSize({ width: 1280, height: 720 });
  const canvas = page.locator(".game canvas.render-surface:visible").first();
  await expect.poll(async () => canvas.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    return Math.abs(element.width - Math.round(rect.width * dpr)) <= 1
      && Math.abs(element.height - Math.round(rect.height * dpr)) <= 1;
  })).toBe(true);
  await page.evaluate(() => document.fullscreenElement && document.exitFullscreen());
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("");
});

test("the in-game fullscreen exit control works for pointer and keyboard input", async ({ page }) => {
  await page.goto("/?hud=diag");
  await page.locator("#fullscreen-toggle").click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
  const exitButton = page.locator("#fullscreen-exit");
  await expect(exitButton).toBeVisible();
  const buttonBounds = await exitButton.boundingBox();
  const hudBounds = await page.locator("#game-hud").boundingBox();
  expect(buttonBounds.y).toBeGreaterThanOrEqual(hudBounds.y + hudBounds.height);

  await page.keyboard.down("b");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "true");
  await exitButton.click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("");
  await expect(page.locator("#tick")).toHaveAttribute("data-look-behind", "false");
  await expect(exitButton).toBeHidden();

  await page.locator("#fullscreen-toggle").click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
  await exitButton.focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("");
});

test("fullscreen request errors leave the game usable", async ({ page }) => {
  await page.goto("/?hud=diag");
  await page.evaluate(() => {
    Object.defineProperty(document.querySelector("#game"), "requestFullscreen", {
      configurable: true,
      value: () => Promise.reject(new Error("permission denied")),
    });
  });
  await page.locator("#fullscreen-toggle").click();
  await expect(page.locator("#fullscreen-status")).toBeVisible();
  await expect(page.locator("#fullscreen-status")).toContainText("Fullscreen could not start");
  await expect(page.locator("#fullscreen-toggle")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#race-canvas")).toBeVisible();
});

test("fullscreen HUD clears the synthetic mirror bounds at narrow and short DPR 2 sizes", async ({ browser }) => {
  for (const { width, height } of [
    { width: 320, height: 600 },
    { width: 320, height: 360 },
    { width: 960, height: 600 },
    { width: 960, height: 360 },
  ]) {
    const context = await browser.newContext({
      viewport: { width, height },
      deviceScaleFactor: 2,
    });
    const page = await context.newPage();
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto("/?practice=local&hud=diag");
    await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
    await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
    await page.locator("#fullscreen-toggle").click();
    await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
    const canvas = page.locator(".game canvas.render-surface:visible").first();
    const geometry = await canvas.evaluate((element) => {
      const canvasRect = element.getBoundingClientRect();
      const gameRect = element.closest(".game").getBoundingClientRect();
      return {
        width: element.width,
        height: element.height,
        cssWidth: canvasRect.width,
        cssHeight: canvasRect.height,
        offsetX: canvasRect.x - gameRect.x,
        offsetY: canvasRect.y - gameRect.y,
      };
    });
    const mirror = await page.evaluate(({ width, height, cssWidth, cssHeight, offsetX, offsetY }) => {
      let mirrorWidth = Math.min(Math.max(1, Math.floor(width / 3)), 512);
      let mirrorHeight = Math.max(1, Math.floor(mirrorWidth * 9 / 16));
      const maxHeight = Math.min(Math.max(1, Math.floor(height / 3)), 256);
      if (mirrorHeight > maxHeight) {
        mirrorHeight = maxHeight;
        mirrorWidth = Math.max(1, Math.floor(mirrorHeight * 16 / 9));
      }
      const margin = Math.min(18, Math.max(4, Math.min(width, height) * 0.025));
      const border = Math.min(5, Math.max(2, Math.min(width, height) * 0.006));
      const left = width - margin - mirrorWidth - border;
      const top = Math.max(height * 0.22, margin + border) - border;
      const canvas = document.querySelector(".game canvas.render-surface:not([hidden])");
      const gameRect = document.querySelector("#game").getBoundingClientRect();
      const sx = cssWidth / width, sy = cssHeight / height;
      return {
        left: gameRect.x + offsetX + left * sx,
        top: gameRect.y + offsetY + top * sy,
        right: gameRect.x + offsetX + (left + mirrorWidth + border * 2) * sx,
        bottom: gameRect.y + offsetY + (top + mirrorHeight + border * 2) * sy,
      };
    }, geometry);
    const hud = await page.locator("#game-hud").boundingBox();
    const exit = await page.locator("#fullscreen-exit").boundingBox();
    const overlap = (a, b) => Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left))
      * Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
    const rect = (box) => ({ left: box.x, top: box.y, right: box.x + box.width, bottom: box.y + box.height });
    expect(overlap(rect(hud), mirror), `${width}x${height} HUD vs mirror ${JSON.stringify({ geometry, hud, mirror })}`).toBe(0);
    expect(overlap(rect(exit), mirror), `${width}x${height} exit vs mirror`).toBe(0);
    expect(overlap(rect(exit), rect(hud)), `${width}x${height} exit vs HUD`).toBe(0);
    expect(pageErrors).toEqual([]);
    await page.evaluate(() => document.exitFullscreen());
    await context.close();
  }
});

test("fullscreen keeps the driving camera and never shows a top-down fallback view", async ({ page }) => {
  await page.goto("/?hud=diag&camera=road");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
  const camera = page.locator("#preview-camera-mode");
  await expect(camera).toHaveValue("road");
  await page.locator("#fullscreen-toggle").click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id ?? "")).toBe("game");
  await page.waitForTimeout(800);
  await expect(camera).toHaveValue("road");
  await expect(page.locator("#fallback-canvas")).toBeHidden();
  await expect(page.locator("#engine-status")).not.toContainText(/fallback/i);
  const options = await camera.locator("option").evaluateAll((nodes) => nodes.map((node) => node.value));
  expect(options).toEqual(["chase", "wide", "close", "road"]);
  await page.evaluate(() => document.fullscreenElement && document.exitFullscreen());
});
