import assert from "node:assert/strict";
import test from "node:test";
import { createLobbyCloseHandler, ifCurrentRoomWork, isCurrentRaceMessage, isCurrentRoomSnapshot, isCurrentRoomWork, matchesReadyAcknowledgement, reconcileSelectOptions, roomConnectionUiState } from "../../web/multiplayer-state.js";

function fakeSelect() {
  return {
    options: [],
    value: "",
    disabled: false,
    replaceChildren(...options) {
      this.replaceCount = (this.replaceCount ?? 0) + 1;
      this.options = options;
      if (!options.some((option) => option.value === this.value)) this.value = options[0]?.value ?? "";
    },
  };
}

function fakeOption({ value, label, disabled }) {
  return { value, textContent: label, disabled: !!disabled };
}

test("room selection refresh preserves option nodes until roster or role options change", () => {
  const track = fakeSelect();
  const cars = fakeSelect();
  const trackOptions = [
    { value: "track-profile-one", label: "Track one" },
    { value: "track-profile-two", label: "Track two · provisional" },
  ];
  const carOptions = [
    { value: "vehicle-profile-one", label: "Car one preview" },
    { value: "vehicle-profile-two", label: "Car two preview" },
  ];
  reconcileSelectOptions(track, trackOptions, "track-profile-one", false, fakeOption);
  reconcileSelectOptions(cars, carOptions, "vehicle-profile-one", false, fakeOption);
  const initialTrackNodes = [...track.options];
  const initialCarNodes = [...cars.options];

  reconcileSelectOptions(track, trackOptions, "track-profile-one", true, fakeOption);
  reconcileSelectOptions(cars, carOptions, "vehicle-profile-two", false, fakeOption);
  assert.equal(track.options.length, initialTrackNodes.length);
  assert.equal(cars.options.length, initialCarNodes.length);
  initialTrackNodes.forEach((option, index) => assert.strictEqual(track.options[index], option));
  initialCarNodes.forEach((option, index) => assert.strictEqual(cars.options[index], option));
  assert.equal(track.replaceCount, 1);
  assert.equal(cars.replaceCount, 1);
  assert.equal(track.disabled, true);
  assert.equal(cars.value, "vehicle-profile-two");

  const changedCars = [...carOptions, {
    value: "vehicle-profile-legacy",
    label: "vehicle-profile-legacy · unsupported for selected track",
    disabled: true,
  }];
  reconcileSelectOptions(cars, changedCars, "vehicle-profile-legacy", true, fakeOption);
  assert.equal(cars.replaceCount, 2);
  assert.notEqual(cars.options[0], initialCarNodes[0]);
  assert.equal(cars.options[2].disabled, true);
  assert.equal(cars.value, "vehicle-profile-legacy");

  reconcileSelectOptions(cars, carOptions, "vehicle-profile-two", false, fakeOption);
  assert.equal(cars.replaceCount, 3);
  const beforeLabelChange = cars.options[0];
  reconcileSelectOptions(cars, [
    { ...carOptions[0], label: "Car one local preview" },
    carOptions[1],
  ], "vehicle-profile-two", false, fakeOption);
  assert.equal(cars.replaceCount, 4);
  assert.notEqual(cars.options[0], beforeLabelChange);
  assert.equal(cars.options[0].textContent, "Car one local preview");

  reconcileSelectOptions(cars, [carOptions[1], carOptions[0]], "vehicle-profile-two", false, fakeOption);
  assert.equal(cars.replaceCount, 5);
  assert.equal(cars.options[0].value, "vehicle-profile-two");
  const disabledCars = [
    { ...carOptions[1], disabled: true },
    carOptions[0],
  ];
  reconcileSelectOptions(cars, disabledCars, "vehicle-profile-two", false, fakeOption);
  assert.equal(cars.replaceCount, 6);
  assert.equal(cars.options[0].disabled, true);

  const changedTrackOptions = [...trackOptions, { value: "track-profile-new", label: "track-profile-new · unavailable on this server" }];
  reconcileSelectOptions(track, changedTrackOptions, "track-profile-new", false, fakeOption);
  assert.equal(track.replaceCount, 2);
  assert.equal(track.value, "track-profile-new");

  reconcileSelectOptions(track, [], "", true, fakeOption);
  assert.equal(track.replaceCount, 3);
  assert.deepEqual(track.options, []);
  assert.equal(track.value, "");
  assert.equal(track.disabled, true);
});

test("room snapshots from a departed room are ignored after a later join", () => {
  assert.equal(isCurrentRoomSnapshot({ race_id: 9 }, 10), false);
  assert.equal(isCurrentRoomSnapshot({ race_id: 10 }, 10), true);
});

test("out-of-order ready acknowledgements cannot update a newer room revision", () => {
  const pending = { raceId: 10, revision: 7, ready: true };
  assert.equal(matchesReadyAcknowledgement({ race_id: 9, selection_revision: 7, ready: true }, pending, 10, 7), false);
  assert.equal(matchesReadyAcknowledgement({ race_id: 10, selection_revision: 6, ready: true }, pending, 10, 7), false);
  assert.equal(matchesReadyAcknowledgement({ race_id: 10, selection_revision: 7, ready: false }, pending, 10, 7), false);
  assert.equal(matchesReadyAcknowledgement({ race_id: 10, selection_revision: 7, ready: true }, pending, 10, 7), true);
});

test("production close handler clears pending actions before refreshing disconnected UI", () => {
  const activeRaceId = 41;
  let state = {
    connected: true,
    authoritative: { race_id: activeRaceId, selection_revision: 8 },
    roomSelectionPending: true,
    pendingReadyRequest: { raceId: activeRaceId, revision: 8, ready: true },
    readyState: true,
    roomTemplateSyncEpoch: 12,
    roomTemplateSyncSignature: "old-signature",
    roomTemplateReadySignature: "old-ready-signature",
    currentRoomTemplateProfiles: [{ profile_id: "vehicle-profile-test" }],
    networkSessionRevision: 7,
  };
  let refreshCount = 0;
  let disabledCreateButton = false;
  const close = createLobbyCloseHandler({
    getState: () => state,
    setState: (next) => { state = next; },
    refreshUi: (next) => {
      refreshCount++;
      disabledCreateButton = !next.connected;
    },
  });
  close();

  assert.equal(state.connected, false);
  assert.equal(state.authoritative, null);
  assert.equal(state.roomSelectionPending, false);
  assert.equal(state.pendingReadyRequest, null);
  assert.equal(state.readyState, false);
  assert.equal(state.roomTemplateSyncEpoch, 13);
  assert.equal(state.roomTemplateSyncSignature, null);
  assert.equal(state.roomTemplateReadySignature, null);
  assert.deepEqual(state.currentRoomTemplateProfiles, []);
  assert.equal(state.networkSessionRevision, 8);
  assert.deepEqual(roomConnectionUiState(state.connected), {
    guidance: "Connection lost. Reload this page to reconnect, then rejoin the room.",
    actionsDisabled: true,
  });
  assert.deepEqual(roomConnectionUiState(true), { guidance: null, actionsDisabled: false });
  assert.equal(refreshCount, 1);
  assert.equal(disabledCreateButton, true);
  // The close transition intentionally preserves the active room identity.
  assert.equal(activeRaceId, 41);
});

test("countdown and reliable events are scoped to the active room", () => {
  assert.equal(isCurrentRaceMessage({ race_id: 9 }, null), false);
  assert.equal(isCurrentRaceMessage({ race_id: 9 }, 10), false);
  assert.equal(isCurrentRaceMessage({ race_id: 10 }, 10), true);
  assert.equal(isCurrentRaceMessage({ race_id: "10" }, 10), false);
});

test("async roster failures are current only for their captured room, revision, and epoch", () => {
  const baseRoom = { raceId: 41, selectionRevision: 6, epoch: 12 };
  assert.equal(isCurrentRoomWork(baseRoom, baseRoom), true);
  let guidance = "";
  assert.equal(ifCurrentRoomWork(baseRoom, baseRoom, () => { guidance = "current failure"; }), true);
  assert.equal(guidance, "current failure");
  for (const current of [
    { ...baseRoom, raceId: 42 },
    { ...baseRoom, selectionRevision: 7 },
    { ...baseRoom, epoch: 13 },
    { ...baseRoom, selectionRevision: null },
  ]) {
    assert.equal(ifCurrentRoomWork(baseRoom, current, () => { guidance = "stale failure"; }), false);
  }
  assert.equal(guidance, "current failure");
});
