export function mirrorVisibilityAfterKeydown(visible, event, editableTarget) {
  if (event.code !== "F7" || event.repeat || event.ctrlKey || event.metaKey || event.altKey || editableTarget) {
    return null;
  }
  return !visible;
}
