import { expect, test } from "@playwright/test";
import { inflateSync } from "node:zlib";

function centerPixel(png) {
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
  const row = rows[Math.floor(height / 2)], i = Math.floor(width / 2) * channels;
  return [...row.subarray(i, i + 3)];
}

test("camera-centered background retains yaw, ignores translation, and stays behind foreground", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu WebGL2 renderer/);
  // Stop the app's frame loop so it cannot overwrite the explicit fixture
  // between its render call and the canvas capture.
  await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
  await page.waitForTimeout(50);
  const canvas = page.locator("canvas.render-surface:visible");
  const render = (yaw, x, y, foreground = false, enabled = true) => page.evaluate(async (args) => {
    const wasm = await import("/pkg/racer_client.js");
    wasm.render_background_test(640, 480, ...args);
  }, [yaw, x, y, foreground, enabled]);
  const frame = async () => canvas.screenshot();
  const pixel = async () => centerPixel(await frame());

  await render(0, 0, 0);
  const eastFrame = await frame();
  const east = await pixel();
  expect(east[0]).toBeGreaterThan(east[1] * 2);
  expect(east[0]).toBeGreaterThan(east[2] * 2);
  expect(east[0]).toBeGreaterThan(220); // unlit material color retains full red channel

  await render(0, 12, -10);
  expect(await frame()).toEqual(eastFrame);

  await render(Math.PI / 2, 0, 0);
  const north = await pixel();
  expect(north[1]).toBeGreaterThan(north[0] * 2);
  expect(north[1]).toBeGreaterThan(north[2] * 2);

  await render(Math.PI, 0, 0);
  const west = await pixel();
  expect(west[2]).toBeGreaterThan(west[0] * 2);
  expect(west[2]).toBeGreaterThan(west[1] * 2);

  await render(0, 0, 0, true);
  const occluded = await pixel();
  expect(occluded[0]).toBeGreaterThan(occluded[1] * 2);
  expect(occluded[2]).toBeGreaterThan(occluded[1] * 2);

  await render(0, 0, 0, false, false);
  const clear = await pixel();
  expect(clear[2]).toBeGreaterThan(clear[0]);
  expect(clear[1]).toBeGreaterThan(clear[0]);
  expect(pageErrors).toEqual([]);
});

test("closed synthetic panorama covers camera pitch and is translation invariant", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu WebGL2 renderer/);
  await page.evaluate(async () => { const raf = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = () => 0; await new Promise((resolve) => raf(() => raf(resolve))); }); // stop the app frame loop and drain its already-queued frame
  await page.waitForTimeout(50);
  const canvas = page.locator("canvas.render-surface:visible");
  const render = (yaw, pitch, x, y) => page.evaluate(async (args) => {
    const wasm = await import("/pkg/racer_client.js");
    wasm.render_horizon_test(640, 480, ...args);
  }, [yaw, pitch, x, y]);
  const frame = async () => canvas.screenshot();
  const pixel = async () => centerPixel(await frame());

  // Sample the middle of the red quadrant, well away from a panel seam.
  await render(Math.PI / 4, 0, 0, 0);
  const eastFrame = await frame();
  const east = await pixel();
  expect(east[0]).toBeGreaterThan(east[1] * 2);
  expect(east[0]).toBeGreaterThan(east[2] * 2);
  await render(Math.PI / 4, 0, 12, -10);
  expect(await frame()).toEqual(eastFrame);

  await render(3 * Math.PI / 4, 0, 0, 0);
  const nextPanel = await pixel();
  expect(nextPanel[1]).toBeGreaterThan(nextPanel[0] * 2);
  expect(nextPanel[1]).toBeGreaterThan(nextPanel[2] * 2);

  await render(Math.PI / 4, 1.2, 0, 0);
  const upper = await pixel();
  expect(upper[0]).toBeGreaterThan(upper[2] * 2);
  expect(upper[1]).toBeGreaterThan(upper[2] * 2);

  await render(Math.PI / 4, -1.2, 0, 0);
  const lower = await pixel();
  expect(lower[0]).toBeGreaterThan(lower[1] * 2);
  expect(lower[2]).toBeGreaterThan(lower[1] * 2);
  expect(pageErrors).toEqual([]);
});
