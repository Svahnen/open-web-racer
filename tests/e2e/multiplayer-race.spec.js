import { expect, test } from "@playwright/test";

// Two live WebGL pages race each other through lobby, join, countdown, driving, lap progress and finish
// on the project-owned test circuit (no imported content). Trace recording is off: it slows software GL a lot.
test.use({ trace: "off" });

const GATES = [[-20, 0], [0, 12], [20, 0], [0, -12]];

/** Page-side bot: steers to the next checkpoint with synthetic key events until the race is finished. */
async function driveToFinish(page) {
  return page.evaluate(async (waypoints) => {
    const tick = document.querySelector("#tick");
    const pressed = new Set();
    const key = (code, down) => {
      if (down === pressed.has(code)) return;
      pressed[down ? "add" : "delete"](code);
      window.dispatchEvent(new KeyboardEvent(down ? "keydown" : "keyup", { code, bubbles: true }));
    };
    const checkpointsSeen = new Set();
    for (let i = 0; i < 2400 && tick.dataset.finished !== "true"; i++) {
      const x = Number(tick.dataset.x), y = Number(tick.dataset.y);
      const heading = Number(tick.dataset.heading), checkpoint = Number(tick.dataset.checkpoint);
      checkpointsSeen.add(checkpoint);
      const speed = Number(document.querySelector("#speed").textContent);
      if (tick.dataset.server !== "true") throw new Error("controller lost authoritative server snapshots");
      const [gx, gy] = waypoints[checkpoint % waypoints.length];
      let error = Math.atan2(gy - y, gx - x) - heading;
      error = ((error + Math.PI) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI) - Math.PI;
      const steer = Math.abs(error) < 0.1 ? null : error > 0 ? "ArrowLeft" : "ArrowRight";
      key("ArrowRight", steer === "ArrowRight"); key("ArrowLeft", steer === "ArrowLeft");
      key("KeyW", speed < 5); key("KeyS", speed > 6);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    for (const code of [...pressed]) key(code, false);
    return { finished: tick.dataset.finished === "true", event: tick.dataset.lastEvent, checkpoints: [...checkpointsSeen] };
  }, GATES);
}

test("two players race the test circuit: both see each other, make lap progress and finish", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once");
  test.setTimeout(420_000);
  const hostContext = await browser.newContext({ viewport: { width: 640, height: 400 } });
  const guestContext = await browser.newContext({ viewport: { width: 640, height: 400 } });
  const host = await hostContext.newPage();
  const guest = await guestContext.newPage();
  const errors = [];
  for (const [name, page] of [["host", host], ["guest", guest]]) page.on("pageerror", (error) => errors.push(`${name}: ${error.message}`));
  await Promise.all([host.goto("/"), guest.goto("/")]);
  for (const page of [host, guest]) await expect(page.locator("#network-status")).toHaveText("Connected to this server", { timeout: 60_000 });
  await host.locator("#network-driver-name").fill("Hostess");
  await guest.locator("#network-driver-name").fill("Guestie");

  // Lobby: the host creates the race, the guest sees it in the list with its settings and joins.
  await host.locator("#race-name").fill("Pair race");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("0");
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  const row = guest.locator(".race-row", { hasText: "Pair race" });
  await expect(row).toBeVisible();
  await expect(row).toContainText("1/8 drivers");
  await row.getByRole("button", { name: "Join" }).click();
  await expect(guest.locator("#room-controls")).toBeVisible();
  await expect(host.locator(".room-roster")).toContainText("Guestie", { timeout: 20_000 });
  await expect(guest.locator(".room-roster")).toContainText("Hostess");
  await expect(host.locator(".race-row", { hasText: "Pair race" })).toContainText("2/8 drivers");

  // Start: nobody races before both are ready.
  await host.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Lobby");
  await guest.getByRole("button", { name: "Ready" }).click();
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  await expect(guest.locator("#room-state")).toHaveText("Racing", { timeout: 30_000 });
  for (const page of [host, guest]) {
    await expect(page.locator("#tick")).toHaveAttribute("data-vehicle-count", "2");
    await expect(page.locator("#tick")).toHaveAttribute("data-remote-vehicle-count", "1");
  }

  // Both cars are drawn for each player: the frame is not blank and the other car moves in the snapshot.
  for (const [name, page] of [["host", host], ["guest", guest]]) {
    const frame = await page.locator("canvas.render-surface:visible").screenshot({ path: test.info().outputPath(`${name}-grid.png`) });
    expect(new Set(frame.subarray(0, 4000)).size).toBeGreaterThan(20);
  }
  const remote = async (page) => [Number(await page.locator("#tick").getAttribute("data-remote-x")), Number(await page.locator("#tick").getAttribute("data-remote-y"))];
  const guestSawHostAt = await remote(guest);
  await host.locator("canvas.render-surface:visible").click();
  await host.keyboard.down("w");
  await expect.poll(async () => {
    const [x, y] = await remote(guest);
    return Math.hypot(x - guestSawHostAt[0], y - guestSawHostAt[1]);
  }, { timeout: 30_000 }).toBeGreaterThan(2);
  await host.keyboard.up("w");
  const hostSawGuestAt = await remote(host);
  await guest.locator("canvas.render-surface:visible").click();
  await guest.keyboard.down("w");
  await expect.poll(async () => {
    const [x, y] = await remote(host);
    return Math.hypot(x - hostSawGuestAt[0], y - hostSawGuestAt[1]);
  }, { timeout: 30_000 }).toBeGreaterThan(2);
  await guest.keyboard.up("w");

  // Both drive the lap at once; each reaches every checkpoint in order and finishes.
  const [hostResult, guestResult] = await Promise.all([driveToFinish(host), driveToFinish(guest)]);
  expect(hostResult.finished, JSON.stringify(hostResult)).toBe(true);
  expect(guestResult.finished, JSON.stringify(guestResult)).toBe(true);
  expect(hostResult.checkpoints.length).toBeGreaterThan(2);
  expect(guestResult.checkpoints.length).toBeGreaterThan(2);
  // The finish event can arrive a moment after the finished flag; each player gets its own finish event.
  const placeOf = async (page) => {
    await expect(page.locator("#tick")).toHaveAttribute("data-last-event", /^finish:\d$/, { timeout: 10_000 });
    return page.locator("#tick").getAttribute("data-last-event");
  };
  expect([await placeOf(host), await placeOf(guest)]).toEqual(["finish:1", "finish:1"]); // the value is the completed laps (1-lap race)
  for (const page of [host, guest]) await expect(page.locator("#room-state")).toHaveText("Finished", { timeout: 10_000 });

  // Leaving cleans the room up for the other player.
  await guest.getByRole("button", { name: "Leave race" }).click();
  await expect(guest.locator("#room-controls")).toBeHidden();
  await expect(host.locator(".race-row", { hasText: "Pair race" })).toContainText("1/8 drivers", { timeout: 20_000 });
  expect(errors).toEqual([]);
  await hostContext.close();
  await guestContext.close();
});
