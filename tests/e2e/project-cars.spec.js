import { expect, test } from "@playwright/test";

// Project-owned demo cars on the project test circuit (no installed game data).
// The cars are drawn by the real renderer, so the checks decode a screenshot
// of the game area and count pixels of the selected paint colour.

const ready = async (page) => {
  await expect(page.locator("#engine-status")).toContainText(/WASM \+ wgpu (WebGPU|WebGL2) renderer · 60 Hz/);
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", /.+/);
};

/** Count screenshot pixels matching `rule` ("blue" | "red" | "orange"). */
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
      red: (r, g, b) => r > 140 && g < 120 && b < 120 && r - g > 60 && r - b > 60 && Math.abs(g - b) < 40,
      orange: (r, g, b) => r > 170 && g > 70 && g < 160 && b < 90 && r - b > 100 && g - b > 30,
    };
    let count = 0;
    for (let i = 0; i < px.length; i += 4) if (rules[rule](px[i], px[i + 1], px[i + 2])) count += 1;
    return count;
  }, { data: png.toString("base64"), rule });
}

test("solo set-up lists four demo cars with paints and draws the chosen paint", async ({ page }) => {
  test.setTimeout(120_000);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/?practice=local&ai=0&skill=1&catchup=1");
  await ready(page);
  await expect(page.locator("#project-car-setup")).toBeVisible();
  await expect(page.locator("#project-car-choice option")).toHaveCount(4);
  await expect(page.locator("#project-paint-choice option")).toHaveCount(8);
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car-count", "4");
  const names = await page.locator("#project-car-choice option").allTextContents();
  expect(new Set(names).size).toBe(4);

  await page.locator("#project-paint-choice").selectOption("harbour-blue");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", "quill-coupe:harbour-blue");
  await expect.poll(() => countPaint(page, "blue"), { timeout: 30_000 }).toBeGreaterThan(150);
  const redWithBlueCar = await countPaint(page, "red"); // tail lamps and HUD text only

  await page.locator("#project-paint-choice").selectOption("signal-red");
  await expect.poll(() => countPaint(page, "red"), { timeout: 30_000 }).toBeGreaterThan(redWithBlueCar + 150);
  expect(await countPaint(page, "blue")).toBeLessThan(40);

  // A different model is a different silhouette in the same paint.
  const coupeShot = await page.locator("#game").screenshot();
  await page.locator("#project-car-choice").selectOption("tumble-rally");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", "tumble-rally:signal-red");
  await expect.poll(() => countPaint(page, "red"), { timeout: 30_000 }).toBeGreaterThan(150);
  const rallyShot = await page.locator("#game").screenshot();
  expect(Buffer.compare(coupeShot, rallyShot)).not.toBe(0);
  expect(pageErrors).toEqual([]);
});

test("the choice is remembered and a query overrides it", async ({ page }) => {
  await page.goto("/?practice=local&ai=0");
  await ready(page);
  await page.locator("#project-car-choice").selectOption("shard-gt");
  await page.locator("#project-paint-choice").selectOption("ember-orange");
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", "shard-gt:ember-orange");
  await expect.poll(() => countPaint(page, "orange"), { timeout: 30_000 }).toBeGreaterThan(150);

  await page.reload();
  await ready(page);
  await expect(page.locator("#project-car-choice")).toHaveValue("shard-gt");
  await expect(page.locator("#project-paint-choice")).toHaveValue("ember-orange");

  await page.goto("/?practice=local&ai=0&car=brisk-roadster&paint=plum");
  await ready(page);
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", "brisk-roadster:plum");
  await page.goto("/?practice=local&ai=0&car=unknown-car&paint=nothing");
  await ready(page);
  await expect(page.locator("#tick")).toHaveAttribute("data-project-car", "quill-coupe:signal-red");
});

test("opponents drive other cars in other colours", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/?practice=local&ai=7&skill=1&catchup=1&car=quill-coupe&paint=signal-red");
  await ready(page);
  await expect(page.locator("#tick")).toHaveAttribute("data-opponent-count", "7");
  const field = (await page.locator("#tick").getAttribute("data-project-field")).split(",");
  expect(field).toHaveLength(8);
  expect(field[0]).toBe("pc-quill-coupe-signal-red");
  expect(new Set(field).size).toBe(8);
  expect(new Set(field.map((id) => id.replace(/-(signal-red|harbour-blue|meadow-green|solar-yellow|ember-orange|graphite|pearl-white|plum)$/, ""))).size).toBe(4);
});
