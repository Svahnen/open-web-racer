import { expect, test } from "@playwright/test";

// Two live WebGL pages: any trace recording (even without screenshots) makes each action several times slower on software GL.
test.use({ trace: "off" });

test("two browser contexts complete an authoritative race and clean up disconnects", async ({ browser, browserName }) => {
  test.skip(browserName !== "chromium", "the live race automation runs once; static client coverage runs in both browsers");
  test.setTimeout(240_000);
  const first = await browser.newContext();
  const second = await browser.newContext();
  const host = await first.newPage();
  const guest = await second.newPage();
  await Promise.all([host.goto("/?hud=diag"), guest.goto("/?hud=diag")]);
  await expect(host.locator("#network-status")).toHaveText("Connected to this server");
  await expect(guest.locator("#network-status")).toHaveText("Connected to this server");
  await expect(host.locator("#hud-state")).toHaveText("LOBBY");
  await expect(host.locator("#hud-lap-card")).toBeHidden();

  await host.locator("#race-name").fill("Browser pair test");
  await host.locator("#race-laps").selectOption("1");
  await host.locator("#race-ai-count").selectOption("0"); // this test races two humans only
  await host.getByRole("button", { name: "Create race" }).click();
  await expect(host.locator("#room-controls")).toBeVisible();
  const guestRace = guest.locator(".race-row", { hasText: "Browser pair test" });
  await expect(guestRace).toBeVisible();
  await guestRace.getByRole("button", { name: "Join" }).click();
  await expect(guest.locator("#room-controls")).toBeVisible();

  await host.evaluate(() => { window.__countdownObserved = new Promise((resolve) => {
    const state = document.querySelector("#room-state");
    const sawCountdown = () => state.textContent.startsWith("Starting");
    if (sawCountdown()) return resolve(true);
    const observer = new MutationObserver(() => {
      if (sawCountdown()) { observer.disconnect(); resolve(true); }
    });
    observer.observe(state, { childList: true, characterData: true, subtree: true });
    setTimeout(() => { observer.disconnect(); resolve(sawCountdown()); }, 30_000); // bounded; resolves as soon as the countdown appears
  }); });
  await host.getByRole("button", { name: "Ready" }).click();
  await guest.getByRole("button", { name: "Ready" }).click();
  const sawCountdown = await host.evaluate(() => window.__countdownObserved);
  expect(sawCountdown).toBe(true);
  await expect(host.locator("#room-state")).toHaveText("Racing", { timeout: 20_000 });
  await expect(guest.locator("#room-state")).toHaveText("Racing", { timeout: 20_000 });
  await expect(host.locator("#hud-state")).toHaveText("RACING");
  await expect(host.locator("#hud-time")).not.toHaveText("—");
  await expect(host.locator("#hud-lap-card")).toBeVisible();

  // Drive host through each project-owned checkpoint using keyboard events.
  // The controller reads server-fed pose and never writes transforms or race state.
  const hostTick = host.locator("#tick");
  // The test car reaches the circuit bounds and stops within a couple of seconds of full throttle,
  // and a slow software-GL host can poll less often than that. So record every authoritative
  // heading/speed update in the page and assert on the recorded history, not on a polled instant.
  const startRecording = (page) => page.evaluate(() => {
    const tickElement = document.querySelector("#tick"), speedElement = document.querySelector("#speed");
    const record = window.__motionRecord = { maxSpeed: 0, headings: [] };
    const sample = () => {
      record.headings.push(Number(tickElement.dataset.heading));
      record.maxSpeed = Math.max(record.maxSpeed, Number(speedElement.textContent) || 0);
    };
    window.__motionObservers?.forEach((observer) => observer.disconnect());
    window.__motionObservers = [new MutationObserver(sample), new MutationObserver(sample)];
    window.__motionObservers[0].observe(tickElement, { attributes: true });
    window.__motionObservers[1].observe(speedElement, { childList: true, characterData: true, subtree: true });
    sample();
  });
  const recorded = (page) => page.evaluate(() => window.__motionRecord);
  const wrap = (delta) => ((delta + Math.PI) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI) - Math.PI;
  const headingExtremes = async (page) => {
    const { headings } = await recorded(page);
    const deltas = headings.map((heading) => wrap(heading - headings[0]));
    return { min: Math.min(...deltas), max: Math.max(...deltas) };
  };

  await startRecording(host);
  await host.keyboard.down("w");
  await host.keyboard.down("ArrowLeft");
  await expect.poll(async () => (await recorded(host)).maxSpeed).toBeGreaterThan(0);
  await expect.poll(async () => (await headingExtremes(host)).max).toBeGreaterThan(0.001);
  await host.keyboard.up("ArrowLeft");
  await host.keyboard.up("w");
  await startRecording(host);
  await host.keyboard.down("w");
  await host.keyboard.down("ArrowRight");
  await expect.poll(async () => (await headingExtremes(host)).min).toBeLessThan(-0.001);
  await host.keyboard.up("ArrowRight");
  await host.keyboard.up("w");

  await startRecording(guest);
  await guest.keyboard.down("w");
  await expect.poll(async () => (await recorded(guest)).maxSpeed).toBeGreaterThan(0);
  await guest.keyboard.up("w");
  await expect(hostTick).toHaveAttribute("data-vehicle-count", "2");
  await expect(guest.locator("#tick")).toHaveAttribute("data-vehicle-count", "2");
  const gates = [[-20, 0], [0, 12], [20, 0], [0, -12]];
  const driveResult = await host.evaluate(async (waypoints) => {
    const tick = document.querySelector("#tick");
    const pressed = new Set();
    const key = (code, down) => {
      if (down === pressed.has(code)) return;
      pressed[down ? "add" : "delete"](code);
      window.dispatchEvent(new KeyboardEvent(down ? "keydown" : "keyup", { code, bubbles: true }));
    };
    for (let i = 0; i < 1800 && tick.dataset.finished !== "true"; i++) {
      const x = Number(tick.dataset.x), y = Number(tick.dataset.y);
      const heading = Number(tick.dataset.heading), checkpoint = Number(tick.dataset.checkpoint);
      const speed = Number(document.querySelector("#speed").textContent);
      if (tick.dataset.server !== "true") throw new Error("controller lost authoritative server snapshots");
      const [gx, gy] = waypoints[checkpoint];
      let error = Math.atan2(gy - y, gx - x) - heading;
      error = ((error + Math.PI) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI) - Math.PI;
      const steer = Math.abs(error) < 0.1 ? null : error > 0 ? "ArrowLeft" : "ArrowRight";
      key("ArrowRight", steer === "ArrowRight"); key("ArrowLeft", steer === "ArrowLeft");
      key("KeyW", speed < 5); key("KeyS", speed > 6);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    for (const code of pressed) key(code, false);
    return { finished: tick.dataset.finished === "true", tick: tick.textContent, event: tick.dataset.lastEvent };
  }, gates);
  expect(driveResult.finished).toBe(true);
  await expect(host.locator("#room-state")).toHaveText("Finished", { timeout: 1000 });
  await expect(hostTick).toHaveAttribute("data-last-event", "finish:1", { timeout: 3000 });
  await guest.keyboard.up("w");
  await expect(host.locator("#tick")).not.toHaveText("0");
  await expect(guest.locator("#tick")).not.toHaveText("0");
  await host.close();
  // The server reacts when the OS reports the closed socket; Chromium tears a page down slowly on a loaded software-GL host.
  await expect(guest.locator("#room-state")).toHaveText("Finished", { timeout: 10_000 });
  await expect(guest.locator("#hud-state")).toHaveText("FINISHED", { timeout: 3000 });
  const finishedSessionTime = await guest.locator("#hud-time").textContent();
  await guest.waitForTimeout(250);
  await expect(guest.locator("#hud-time")).toHaveText(finishedSessionTime);
  await expect(guest.locator(".race-row", { hasText: "Browser pair test" })).toContainText("1/8 drivers");
  await guest.getByRole("button", { name: "Leave race" }).click();
  await expect(guest.locator("#room-controls")).toBeHidden();
  await expect(guest.locator(".race-row", { hasText: "Browser pair test" })).toHaveCount(0);
  await second.close();
});
