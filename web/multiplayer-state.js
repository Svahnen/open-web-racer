export function isCurrentRoomSnapshot(message, activeRaceId) {
  return Number.isInteger(message?.race_id) && message.race_id === activeRaceId;
}

export function isCurrentRaceMessage(message, activeRaceId) {
  return Number.isInteger(message?.race_id) && message.race_id === activeRaceId;
}

export function isCurrentRoomWork(token, current) {
  return !!token
    && token.raceId === current?.raceId
    && token.selectionRevision === current?.selectionRevision
    && token.epoch === current?.epoch;
}

export function ifCurrentRoomWork(token, current, action) {
  if (!isCurrentRoomWork(token, current)) return false;
  action();
  return true;
}

export function matchesReadyAcknowledgement(message, pending, activeRaceId, currentSelectionRevision) {
  return !!pending
    && pending.raceId === activeRaceId
    && message?.race_id === activeRaceId
    && message.selection_revision === pending.revision
    && message.selection_revision === currentSelectionRevision
    && message.ready === pending.ready;
}

export function applyLobbyDisconnect(state) {
  return {
    ...state,
    connected: false,
    authoritative: null,
    roomSelectionPending: false,
    pendingReadyRequest: null,
    readyState: false,
    roomTemplateSyncEpoch: state.roomTemplateSyncEpoch + 1,
    roomTemplateSyncSignature: null,
    roomTemplateReadySignature: null,
    currentRoomTemplateProfiles: [],
    networkSessionRevision: state.networkSessionRevision + 1,
  };
}

export function createLobbyCloseHandler({ getState, setState, refreshUi }) {
  return () => {
    const disconnected = applyLobbyDisconnect(getState());
    setState(disconnected);
    refreshUi(disconnected);
  };
}

export function roomConnectionUiState(connected) {
  return connected
    ? { guidance: null, actionsDisabled: false }
    : {
      guidance: "Connection lost. Reload this page to reconnect, then rejoin the room.",
      actionsDisabled: true,
    };
}

export function reconcileSelectOptions(select, optionSpecs, selectedValue, disabled, createOption) {
  const currentOptions = [...select.options];
  const sameOptions = currentOptions.length === optionSpecs.length
    && currentOptions.every((option, index) => {
      const spec = optionSpecs[index];
      return option.value === spec.value
        && option.textContent === spec.label
        && option.disabled === !!spec.disabled;
    });
  if (!sameOptions) select.replaceChildren(...optionSpecs.map(createOption));
  if (select.value !== selectedValue) select.value = selectedValue;
  if (select.disabled !== disabled) select.disabled = disabled;
}
