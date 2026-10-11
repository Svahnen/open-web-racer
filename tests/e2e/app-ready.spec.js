import { expect, test } from "@playwright/test";
import { inflateSync } from "node:zlib";

function decodePng(image) {
  let offset = 8, width = 0, height = 0, channels = 0;
  const data = [];
  while (offset < image.length) {
    const length = image.readUInt32BE(offset);
    const type = image.toString("ascii", offset + 4, offset + 8);
    const payload = image.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = payload.readUInt32BE(0); height = payload.readUInt32BE(4);
      channels = payload[9] === 6 ? 4 : payload[9] === 2 ? 3 : 0;
    }
    if (type === "IDAT") data.push(payload);
    offset += length + 12;
  }
  if (!channels) return null;
  const packed = inflateSync(Buffer.concat(data));
  const rowBytes = width * channels;
  const rows = [];
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

// Samples an 11x11 patch around a fraction of the frame and requires nearly all pixels to be
// within `tolerance` of an expected sRGB byte color. The project-owned fixture scene is
// lit and textured, so expectations are measured output bytes, not raw material colors.
function framePatchHasColor(frame, xRatio, yRatio, expected, tolerance = 14) {
  const centerX = Math.round(frame.width * xRatio), centerY = Math.round(frame.height * yRatio);
  let matches = 0, samples = 0;
  for (let y = Math.max(0, centerY - 5); y <= Math.min(frame.height - 1, centerY + 5); y++) {
    for (let x = Math.max(0, centerX - 5); x <= Math.min(frame.width - 1, centerX + 5); x++) {
      const i = x * frame.channels, pixel = frame.rows[y];
      samples++;
      if (expected.every((value, channel) => Math.abs(pixel[i + channel] - value) <= tolerance)) matches++;
    }
  }
  return matches / samples >= 0.9;
}

function frameRegionColorCount(frame, [x0, y0, x1, y1], expected, tolerance = 14) {
  let matches = 0;
  for (let y = Math.round(frame.height * y0); y < Math.round(frame.height * y1); y++) {
    for (let x = Math.round(frame.width * x0); x < Math.round(frame.width * x1); x++) {
      const i = x * frame.channels, pixel = frame.rows[y];
      if (expected.every((value, channel) => Math.abs(pixel[i + channel] - value) <= tolerance)) matches++;
    }
  }
  return matches;
}

test("static browser client renders WASM and keyboard input advances its fixed-step simulation", async ({ page }, testInfo) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "warning") console.info(`[${testInfo.project.name}] ${message.text()}`);
  });
  await page.goto("/?practice=local&hud=diag");
  await expect(page.getByRole("heading", { name: "Open Web Racer" })).toBeVisible();
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#hud-state")).toHaveText("PRACTICE");
  await expect(page.locator("#hud-time-label")).toHaveText("SESSION");
  await expect(page.locator("#hud-time")).not.toHaveText("—");
  await expect(page.locator("#hud-lap-card")).toBeVisible();
  console.info(`[${testInfo.project.name}] renderer: ${await page.locator("#engine-status").textContent()}; navigator.gpu: ${await page.evaluate(() => Boolean(navigator.gpu))}`);
  await expect(page.locator("canvas.render-surface:visible")).toHaveCount(1);
  const canvas = page.locator("canvas.render-surface:visible");
  await expect.poll(async () => canvas.evaluate((element) => element.width * element.height)).toBeGreaterThan(0);
  const initialWidth = await canvas.evaluate((element) => element.width);
  await page.setViewportSize({ width: 1024, height: 768 });
  await expect.poll(async () => canvas.evaluate((element) => element.width)).not.toBe(initialWidth);
  await expect.poll(async () => Number(await page.locator("#tick").textContent())).toBeGreaterThan(3);
  await page.screenshot({ path: testInfo.outputPath(`phase2-${testInfo.project.name}.png`), fullPage: true });
  // The HTML HUD, toast, controls hint (text over the lower frame), and rear-mirror inset are drawn over/into the canvas. Hide the HTML
  // overlays for the capture and sample only regions outside the mirror inset (upper right).
  await page.addStyleTag({ content: ".hud, .hud-race, .hud-canvas, .toast, .route-map-panel, .controls-hint { visibility: hidden !important; }" });
  const frameImage = await canvas.screenshot({ path: testInfo.outputPath(`phase2-frame-${testInfo.project.name}.png`) });
  const frame = decodePng(frameImage);
  expect(frame).not.toBeNull();
  expect(framePatchHasColor(frame, 0.04, 0.60, [126, 161, 126])).toBe(true); // ground (left of road)
  expect(framePatchHasColor(frame, 0.76, 0.80, [126, 161, 126])).toBe(true); // ground (right of road)
  expect(framePatchHasColor(frame, 0.35, 0.88, [115, 118, 123], 16)).toBe(true); // asphalt (textured)
  expect(framePatchHasColor(frame, 0.29, 0.80, [222, 219, 203])).toBe(true); // left curb
  expect(framePatchHasColor(frame, 0.72, 0.80, [222, 219, 203])).toBe(true); // right curb
  expect(frameRegionColorCount(frame, [0.42, 0.52, 0.58, 0.72], [212, 99, 94])).toBeGreaterThan(150); // default project demo car (red paint)
  await expect(page.locator("#speed")).toHaveText("0.0");

  await canvas.click();
  await page.keyboard.down("w");
  await expect.poll(async () => Number(await page.locator("#speed").textContent())).toBeGreaterThan(0);
  await page.keyboard.up("w");
  await expect.poll(async () => Number(await page.locator("#tick").textContent())).toBeGreaterThan(10);
  await page.getByRole("button", { name: "Reset car" }).click();
  await expect(page.locator("#speed")).toHaveText("0.0");
  expect(pageErrors).toEqual([]);
});

test("HUD fits a narrow viewport while retaining hidden tick telemetry", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/?practice=local&hud=diag");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#hud-state")).toHaveText("PRACTICE");
  await expect.poll(async () => Number(await page.locator("#tick").textContent())).toBeGreaterThan(3);
  const layout = await page.locator(".hud").evaluate((hud) => {
    const cards = Array.from(hud.querySelectorAll(".hud-card:not([hidden])"));
    const overflowElements = Array.from(document.querySelectorAll("body *"))
      .map((element) => ({ tag: element.tagName, id: element.id, className: String(element.className), rect: element.getBoundingClientRect(), scrollWidth: element.scrollWidth, clientWidth: element.clientWidth }))
      .filter((element) => element.rect.right > document.documentElement.clientWidth + 1)
      .slice(0, 8)
      .map(({ tag, id, className, rect, scrollWidth, clientWidth }) => ({ tag, id, className, left: rect.left, right: rect.right, width: rect.width, scrollWidth, clientWidth }));
    return {
      viewportWidth: document.documentElement.clientWidth,
      documentWidth: document.documentElement.scrollWidth,
      hudWidth: hud.clientWidth,
      bodyWidth: document.body.clientWidth,
      bodyScrollWidth: document.body.scrollWidth,
      overflowElements,
      cards: cards.map((card) => ({ left: card.offsetLeft, right: card.offsetLeft + card.offsetWidth })),
      telemetryVisible: document.querySelector("#tick").getBoundingClientRect().width > 2,
    };
  });
  expect(layout.documentWidth).toBeLessThanOrEqual(layout.viewportWidth);
  expect(layout.cards.at(-1).right).toBeLessThanOrEqual(layout.hudWidth);
  expect(layout.telemetryVisible).toBe(false);
});

test("route map draws a synthetic ordered path and pose, then hides when path is absent", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#route-map-panel")).toBeHidden();
  const result = await page.evaluate(async () => {
    const { createRouteMapView, setRouteMapFromSceneDescriptor } = await import("/hud-model.js");
    const wasm = await import("/pkg/racer_client.js");
    const panel = document.querySelector("#route-map-panel");
    const canvas = document.querySelector("#route-map");
    const game = document.querySelector(".game");
    const view = createRouteMapView({ panel, canvas, game });
    window.__testRouteMapView = view;
    const descriptor = { schema_version: 0, preview_reference_path_scene: [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0]] };
    Object.assign(descriptor, {
      coordinate_system: { source_scale_m: 1, source_to_scene_matrix: [[1, 0, 0], [0, 1, 0], [0, 0, 1]] },
      meshes: [{ vertices: [
        { position: [-1, 0, 0], normal: [0, 0, 1], uv: [0, 0], material: 0 },
        { position: [1, 0, 0], normal: [0, 0, 1], uv: [1, 0], material: 0 },
        { position: [0, 2, 0], normal: [0, 0, 1], uv: [0.5, 1], material: 0 },
      ], indices: [0, 1, 2] }],
      materials: [{ name: "synthetic-track", tint: [1, 1, 1, 1], texture: 0, uv_offset: [0, 0], uv_scale: [1, 1], wrap: "clamp" }],
      textures: { width: 1, height: 1, layers: ["tex_0"] },
      instances: [
        { mesh_index: 0, translation: [0, 0, 0], heading: 0, scale: [1, 1, 1], role: "static" },
        { mesh_index: 0, translation: [0, 0, 0], heading: 0, scale: [1, 1, 1], role: "vehicle-local-dynamic" },
      ],
      group_metadata: { track_mesh_indices: [0], spawn_pose_mapping: {
        anchor_source_position_m: [0, 0, 0], anchor_scene_position_m: [0, 0, 0],
        anchor_source_forward: [1, 0, 0], anchor_scene_forward: [1, 0, 0], confidence: "synthetic",
      } },
    });
    wasm.install_scene_descriptor(JSON.stringify(descriptor), [new Uint8Array([255, 255, 255, 255])], 1, false);
    const available = setRouteMapFromSceneDescriptor(view, descriptor);
    view.draw({ x: 5, y: 5, heading: 0 });
    const context = canvas.getContext("2d");
    const east = context.getImageData(0, 0, canvas.width, canvas.height).data;
    view.draw({ x: 5, y: 5, heading: Math.PI / 2 });
    const north = context.getImageData(0, 0, canvas.width, canvas.height).data;
    view.draw({ x: 8, y: 3, heading: 0 });
    const moved = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let changed = 0, markerMoved = 0, routePixels = 0;
    for (let i = 0; i < east.length; i += 4) {
      if (east[i] !== north[i] || east[i + 1] !== north[i + 1] || east[i + 2] !== north[i + 2]) changed++;
      if (east[i] !== moved[i] || east[i + 1] !== moved[i + 1] || east[i + 2] !== moved[i + 2]) markerMoved++;
      if (east[i] > 150 && east[i + 1] > 180 && east[i + 2] > 160) routePixels++;
    }
    view.draw(null);
    const noMarker = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let markerRemoved = 0;
    for (let i = 0; i < east.length; i += 4) {
      if (east[i] !== noMarker[i] || east[i + 1] !== noMarker[i + 1] || east[i + 2] !== noMarker[i + 2]) markerRemoved++;
    }
    const panelRect = panel.getBoundingClientRect(), gameRect = game.getBoundingClientRect();
    const layout = {
      panelRight: panelRect.right, gameRight: gameRect.right,
      panelBottom: panelRect.bottom, gameBottom: gameRect.bottom,
      mapHeight: canvas.getBoundingClientRect().height,
      mapMinHeight: getComputedStyle(canvas).minHeight,
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
    };
    return { available, changed, markerMoved, routePixels, markerRemoved, layout };
  });
  expect(result.available).toBe(true);
  expect(result.changed).toBeGreaterThan(0);
  expect(result.markerMoved).toBeGreaterThan(0);
  expect(result.routePixels).toBeGreaterThan(0);
  expect(result.markerRemoved).toBeGreaterThan(0);
  expect(result.layout.panelRight).toBeLessThanOrEqual(result.layout.gameRight);
  expect(result.layout.panelBottom).toBeLessThanOrEqual(result.layout.gameBottom);
  expect(result.layout.mapHeight).toBeLessThan(100);
  expect(result.layout.mapMinHeight).toBe("0px");
  expect(result.layout.documentWidth).toBeLessThanOrEqual(result.layout.viewportWidth);

  await page.setViewportSize({ width: 600, height: 800 });
  const desktopLayout = await page.evaluate(() => {
    window.__testRouteMapView.draw({ x: 5, y: 5, heading: Math.PI });
    const panelRect = document.querySelector("#route-map-panel").getBoundingClientRect();
    const gameRect = document.querySelector(".game").getBoundingClientRect();
    return { panelRight: panelRect.right, gameRight: gameRect.right, mapHeight: document.querySelector("#route-map").getBoundingClientRect().height };
  });
  expect(desktopLayout.panelRight).toBeLessThanOrEqual(desktopLayout.gameRight);
  expect(desktopLayout.mapHeight).toBeLessThan(120);
  const absentHidden = await page.evaluate(async () => {
    const { setRouteMapFromSceneDescriptor } = await import("/hud-model.js");
    return !setRouteMapFromSceneDescriptor(window.__testRouteMapView, { schema_version: 0 })
      && document.querySelector("#route-map-panel").hidden;
  });
  expect(absentHidden).toBe(true);
});

test("browser gamepad right-axis steers right in the local simulation", async ({ page }) => {
  await page.addInitScript(() => {
    const pad = { connected: true, axes: [0, 0], buttons: Array.from({ length: 8 }, () => ({ pressed: false, value: 0 })) };
    Object.defineProperty(navigator, "getGamepads", { configurable: true, value: () => [pad] });
    window.__testGamepad = pad;
  });
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await page.evaluate(() => { window.__testGamepad.buttons[7].value = 1; });
  await expect.poll(async () => Number(await page.locator("#speed").textContent())).toBeGreaterThan(0);
  await page.evaluate(() => { window.__testGamepad.buttons[7].value = 0; });
  const headingBeforeSteer = Number(await page.locator("#tick").getAttribute("data-heading"));
  await page.evaluate(() => { window.__testGamepad.axes[0] = 0.7; });
  await expect.poll(async () => {
    const heading = Number(await page.locator("#tick").getAttribute("data-heading"));
    return ((heading - headingBeforeSteer + Math.PI) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI) - Math.PI;
  }).toBeLessThan(-0.001);
});

test("Left and right arrows steer; A and Z shift", async ({ page }) => {
  await page.goto("/?practice=local");
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  const canvas = page.locator("canvas.render-surface:visible");
  await canvas.click();
  await page.keyboard.down("w");
  await expect.poll(async () => Number(await page.locator("#speed").textContent())).toBeGreaterThan(5);
  await page.keyboard.up("w");

  const heading = () => page.locator("#tick").getAttribute("data-heading").then(Number);
  const headingDelta = (after, before) => ((after - before + Math.PI) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI) - Math.PI;
  for (const key of ["ArrowLeft"]) {
    const before = await heading();
    await page.keyboard.down(key);
    await expect.poll(async () => headingDelta(await heading(), before)).toBeGreaterThan(0.001);
    await page.keyboard.up(key);
  }
  for (const key of ["ArrowRight"]) {
    const before = await heading();
    await page.keyboard.down(key);
    await expect.poll(async () => headingDelta(await heading(), before)).toBeLessThan(-0.001);
    await page.keyboard.up(key);
  }
});
