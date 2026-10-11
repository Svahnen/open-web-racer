import assert from "node:assert/strict";
import test from "node:test";
import { CONTROLS_HINT, CONTROLS_HINT_MANUAL, buildHudModel, controlsHintFor, gearLabel } from "../../web/hud-model.js";
import { parseOpponentParams, soloQuery } from "../../web/opponent-settings.js";
import {
  TRANSMISSION_AUTOMATIC,
  TRANSMISSION_MANUAL,
  clampTransmission,
  isManual,
  parseTransmissionParam,
  setTransmissionMessage,
  transmissionLabel,
  transmissionQuery,
  transmissionSummary,
} from "../../web/transmission-setting.js";

test("the gearbox mode defaults to automatic and only accepts the two wire values", () => {
  assert.equal(clampTransmission(undefined), TRANSMISSION_AUTOMATIC);
  assert.equal(clampTransmission(""), TRANSMISSION_AUTOMATIC);
  assert.equal(clampTransmission("Manual"), TRANSMISSION_AUTOMATIC);
  assert.equal(clampTransmission("manual"), TRANSMISSION_MANUAL);
  assert.equal(isManual("manual"), true);
  assert.equal(isManual("automatic"), false);
  assert.equal(transmissionLabel("manual"), "Manual");
  assert.equal(transmissionLabel(null), "Automatic");
  assert.match(transmissionSummary("manual"), /Manual gearbox/);
  assert.match(transmissionSummary("automatic"), /Automatic gearbox/);
});

test("solo links carry the gearbox only when manual, so old URLs keep their meaning", () => {
  assert.equal(parseTransmissionParam("?practice=local"), "automatic");
  assert.equal(parseTransmissionParam("?practice=local&gearbox=manual"), "manual");
  assert.equal(parseTransmissionParam("?gearbox=sequential"), "automatic");
  assert.equal(transmissionQuery("manual"), "&gearbox=manual");
  assert.equal(transmissionQuery("automatic"), "");
  const base = { count: 3, skill: 0, catchUp: false };
  assert.equal(soloQuery(base), "practice=local&ai=3&skill=0&catchup=0");
  assert.equal(soloQuery({ ...base, transmission: "manual" }), "practice=local&ai=3&skill=0&catchup=0&gearbox=manual");
  assert.equal(soloQuery({ ...base, transmission: "manual", track: "alpine-1" }), "practice=local&ai=3&skill=0&catchup=0&gearbox=manual&track=alpine-1");
  // The opponent settings parser ignores the extra parameter.
  assert.deepEqual(parseOpponentParams("?practice=local&ai=3&skill=0&catchup=0&gearbox=manual"), { ...base, laps: 1 });
  assert.equal(parseTransmissionParam(`?${soloQuery({ ...base, transmission: "manual" })}`), "manual");
});

test("the room message matches the protocol enum and never carries an unknown value", () => {
  assert.deepEqual(setTransmissionMessage("manual"), { set_transmission: { transmission: "manual", auto_reverse: false } });
  assert.deepEqual(setTransmissionMessage("manual", true), { set_transmission: { transmission: "manual", auto_reverse: true } });
  assert.deepEqual(setTransmissionMessage("automatic"), { set_transmission: { transmission: "automatic", auto_reverse: false } });
  assert.deepEqual(setTransmissionMessage("bogus"), { set_transmission: { transmission: "automatic", auto_reverse: false } });
});

test("the controls hint mentions manual shifting only when manual is selected", () => {
  assert.equal(controlsHintFor("automatic"), CONTROLS_HINT);
  assert.equal(controlsHintFor(undefined), CONTROLS_HINT);
  assert.equal(controlsHintFor("manual"), CONTROLS_HINT_MANUAL);
  assert.match(CONTROLS_HINT_MANUAL, /Manual gearbox/);
  assert.match(CONTROLS_HINT_MANUAL, /A shifts up, Z shifts down/);
  assert.doesNotMatch(CONTROLS_HINT, /Manual gearbox/);
});

test("the HUD shows the gear in both modes (digits, N and R)", () => {
  assert.equal(gearLabel(1), "1");
  assert.equal(gearLabel(6), "6");
  assert.equal(gearLabel(251), "N");
  assert.equal(gearLabel(250), "R");
  for (const gear of [251, 3]) {
    const model = buildHudModel({
      connected: false,
      localPracticeMode: true,
      openDrive: false,
      snapshot: { speed: 12, gear, rpm01: 0.5 },
      elapsedTicks: 0,
    });
    assert.equal(model.gear, gearLabel(gear));
  }
});
