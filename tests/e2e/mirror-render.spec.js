import { expect, test } from "@playwright/test";
import { inflateSync } from "node:zlib";

function decodePng(png) {
  let offset = 8, width = 0, height = 0, channels = 0;
  const chunks = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const data = png.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0); height = data.readUInt32BE(4);
      channels = data[9] === 6 ? 4 : data[9] === 2 ? 3 : 0;
    }
    if (type === "IDAT") chunks.push(data);
    offset += length + 12;
  }
  if (!channels) throw new Error("unsupported screenshot PNG format");
  const packed = inflateSync(Buffer.concat(chunks));
  const rowBytes = width * channels, rows = [];
  for (let y = 0; y < height; y++) {
    const start = y * (rowBytes + 1), filter = packed[start];
    const row = Buffer.from(packed.subarray(start + 1, start + 1 + rowBytes));
    const above = rows[y - 1];
    for (let i = 0; i < rowBytes; i++) {
      const left = i >= channels ? row[i - channels] : 0;
      const up = above?.[i] ?? 0;
      const upperLeft = i >= channels ? (above?.[i - channels] ?? 0) : 0;
      if (filter === 1) row[i] = (row[i] + left) & 255;
      else if (filter === 2) row[i] = (row[i] + up) & 255;
      else if (filter === 3) row[i] = (row[i] + Math.floor((left + up) / 2)) & 255;
      else if (filter === 4) {
        const p = left + up - upperLeft;
        const a = Math.abs(p - left), b = Math.abs(p - up), c = Math.abs(p - upperLeft);
        row[i] = (row[i] + (a <= b && a <= c ? left : b <= c ? up : upperLeft)) & 255;
      }
    }
    rows.push(row);
  }
  return { width, height, channels, rows };
}

function sample(image, xFraction, yFraction) {
  const x = Math.floor(image.width * xFraction), y = Math.floor(image.height * yFraction);
  const i = x * image.channels;
  return [...image.rows[y].subarray(i, i + 3)];
}

test("rear-view inset uses a second camera, honors occlusion/up-axis, and sorts transparent layers", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 960, height: 720 });
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
  await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
  await page.waitForTimeout(50);
  const canvas = page.locator("canvas.render-surface:visible");
  const render = async ({ upX = 0, upY = 0, occluder = false, transparent = false, reverse = false } = {}) => {
    await page.evaluate(async (args) => {
      const wasm = await import("/pkg/racer_client.js");
      wasm.render_rear_mirror_test(640, 480, ...args);
    }, [upX, upY, occluder, transparent, reverse]);
    return decodePng(await canvas.screenshot());
  };

  const base = await render();
  const primaryFront = sample(base, 0.5, 0.5);
  const rearView = sample(base, 0.815, 0.34);
  const rearLeft = sample(base, 0.735, 0.34);
  const rearRight = sample(base, 0.90, 0.34);
  expect(primaryFront[1]).toBeGreaterThan(primaryFront[2] * 2);
  expect(Math.abs(rearView[0] - rearView[2])).toBeGreaterThan(100); // local magenta car is absent
  expect(rearLeft[2]).toBeGreaterThan(rearLeft[0] * 2);
  expect(rearRight[0]).toBeGreaterThan(rearRight[2] * 2);
  let otherVehiclePixels = 0;
  for (let y = Math.floor(base.height * 0.2); y < Math.floor(base.height * 0.5); y++) {
    for (let x = Math.floor(base.width * 0.64); x < base.width; x++) {
      const pixel = base.rows[y], i = x * base.channels;
      if (pixel[i] > pixel[i + 1] && pixel[i + 1] > pixel[i + 2] * 2 && pixel[i] > 80) otherVehiclePixels++;
    }
  }
  expect(otherVehiclePixels).toBeGreaterThan(20);

  const blocked = sample(await render({ occluder: true }), 0.815, 0.34);
  expect(blocked[0]).toBeGreaterThan(blocked[2] * 2);
  expect(blocked[1]).toBeGreaterThan(blocked[2] * 2);

  const level = await render();
  const tilted = await render({ upY: 0.48 });
  expect(sample(tilted, 0.5, 0.5)).toEqual(primaryFront);
  let attitudePixelsChanged = 0;
  for (let y = 0; y < Math.floor(level.height * 0.35); y++) {
    for (let x = Math.floor(level.width * 0.64); x < level.width; x++) {
      const a = level.rows[y], b = tilted.rows[y], i = x * level.channels;
      if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]) attitudePixelsChanged++;
    }
  }
  expect(attitudePixelsChanged).toBeGreaterThan(100);
  let primaryPixelsChanged = 0;
  for (let y = 0; y < level.height; y++) {
    for (let x = 0; x < Math.floor(level.width * 0.64); x++) {
      const a = level.rows[y], b = tilted.rows[y], i = x * level.channels;
      if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]) primaryPixelsChanged++;
    }
  }
  expect(primaryPixelsChanged).toBe(0);

  const alphaForward = sample(await render({ transparent: true }), 0.815, 0.34);
  const alphaReverse = sample(await render({ transparent: true, reverse: true }), 0.815, 0.34);
  expect(alphaForward[1]).toBeGreaterThan(alphaForward[0]);
  expect(alphaForward[0]).toBeGreaterThan(alphaForward[2]);
  expect(alphaReverse.map((value, index) => Math.abs(value - alphaForward[index]))).toEqual([0, 0, 0]);
  expect(pageErrors).toEqual([]);
});

test("rear-view inset clears the HTML HUD at narrow and desktop DPR 1/2", async ({ browser }) => {
  for (const deviceScaleFactor of [1, 2]) {
    for (const width of [320, 960]) {
      const context = await browser.newContext({
        viewport: { width, height: 720 },
        deviceScaleFactor,
      });
      const page = await context.newPage();
      const pageErrors = [];
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.goto("/?practice=local");
      await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
      await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
      const canvas = page.locator("canvas.render-surface:visible");
      const game = page.locator(".game");
      const geometry = await canvas.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        const gameRect = element.closest(".game").getBoundingClientRect();
        return {
          width: element.width,
          height: element.height,
          cssWidth: rect.width,
          cssHeight: rect.height,
          offsetX: rect.x - gameRect.x,
          offsetY: rect.y - gameRect.y,
          gameWidth: gameRect.width,
          gameHeight: gameRect.height,
        };
      });
      await page.evaluate(async ({ width, height }) => {
        const wasm = await import("/pkg/racer_client.js");
        wasm.render_rear_mirror_test(width, height, 0, 0, false, false, false);
      }, { width: geometry.width, height: geometry.height });
      const canvasImage = decodePng(await canvas.screenshot());
      const gameImage = decodePng(await game.screenshot());
      let mirrorWidth = Math.min(Math.max(1, Math.floor(geometry.width / 3)), 512);
      let mirrorHeight = Math.max(1, Math.floor(mirrorWidth * 9 / 16));
      const maxHeight = Math.min(Math.max(1, Math.floor(geometry.height / 3)), 256);
      if (mirrorHeight > maxHeight) {
        mirrorHeight = maxHeight;
        mirrorWidth = Math.max(1, Math.floor(mirrorHeight * 16 / 9));
      }
      const margin = Math.min(18, Math.max(4, Math.min(geometry.width, geometry.height) * 0.025));
      const left = geometry.width - margin - mirrorWidth;
      const top = Math.max(geometry.height * 0.22, margin + Math.min(5, Math.max(2, Math.min(geometry.width, geometry.height) * 0.006)));
      const xFraction = (left + mirrorWidth * 0.5) / geometry.width;
      const yFraction = (top + mirrorHeight * 0.5) / geometry.height;
      const expected = sample(canvasImage, xFraction, yFraction);
      const gameX = Math.floor((geometry.offsetX + xFraction * geometry.cssWidth) * gameImage.width / geometry.gameWidth);
      const gameY = Math.floor((geometry.offsetY + yFraction * geometry.cssHeight) * gameImage.height / geometry.gameHeight);
      const actual = [...gameImage.rows[gameY].subarray(gameX * gameImage.channels, gameX * gameImage.channels + 3)];
      expect(actual.map((value, index) => Math.abs(value - expected[index]))).toEqual([0, 0, 0]);
      expect(pageErrors).toEqual([]);
      await context.close();
    }
  }
});

test("mirror cache bounds reject bad replacements transactionally and clear on valid replacement", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 960, height: 720 });
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer/);
  await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
  await page.waitForTimeout(50);
  const canvas = page.locator("canvas.render-surface:visible");
  const renderStage = async (stage) => {
    await page.evaluate(async ({ width, height, stage }) => {
      const wasm = await import("/pkg/racer_client.js");
      wasm.render_mirror_cache_lifecycle_test(width, height, stage);
    }, { width: 640, height: 480, stage });
    return decodePng(await canvas.screenshot());
  };

  const baseline = await renderStage(0);
  const oversizedRejected = await renderStage(1);
  const transformRejected = await renderStage(2);
  const sameRows = (a, b) => Buffer.concat(a.rows).equals(Buffer.concat(b.rows)); // avoids a huge matcher diff on mismatch
  expect(sameRows(oversizedRejected, baseline)).toBe(true);
  expect(sameRows(transformRejected, baseline)).toBe(true);

  const replaced = await renderStage(3);
  const previousStaticPixel = sample(baseline, 0.815, 0.34);
  const replacedPixel = sample(replaced, 0.815, 0.34);
  expect(replacedPixel[2]).toBeGreaterThan(replacedPixel[0]);
  expect(previousStaticPixel.map((value, index) => Math.abs(value - replacedPixel[index]))
    .some((difference) => difference > 30)).toBe(true);
  expect(pageErrors).toEqual([]);
});
