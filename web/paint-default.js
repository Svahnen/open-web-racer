// Paint helpers used when no `paint` plugin hook is registered: a single
// unpainted colour. Same call shape as the `paint` hook (docs/PLUGINS.md).

export const DEFAULT_PAINT = Object.freeze({
  paintIdForColour: (colour) => (colour === 0 ? "paint-colour-0" : null),
  paintColourFromId: (paintId) => (paintId === "paint-colour-0" ? 0 : null),
  paintChoiceCount: () => 1,
  clampPaintColour: () => 0,
  paintChoiceOptions: () => [{ value: "paint-colour-0", label: "Default" }],
  paintTableBytes: () => new Uint8Array(0),
  carArchiveAPath: () => null,
  variantKey: (profileId, fingerprint, colour) => `${profileId}:${fingerprint}:${Number.isInteger(colour) ? colour : ""}`,
  resolveRenderColour(registered, profileId, fingerprint, wanted) {
    const exact = Number.isInteger(wanted) ? wanted : null;
    if (registered.has(DEFAULT_PAINT.variantKey(profileId, fingerprint, exact))) return exact;
    const prefix = `${profileId}:${fingerprint}:`;
    for (const key of registered) {
      if (!key.startsWith(prefix)) continue;
      const rest = key.slice(prefix.length);
      return rest === "" ? null : Number(rest);
    }
    return undefined;
  },
  requiredPaintVariants(vehicles) {
    const seen = new Map();
    for (const vehicle of vehicles) {
      const colour = DEFAULT_PAINT.paintColourFromId(vehicle.paint_id);
      const key = DEFAULT_PAINT.variantKey(vehicle.vehicle_profile_id, vehicle.vehicle_fingerprint, colour);
      if (!seen.has(key)) seen.set(key, { profile_id: vehicle.vehicle_profile_id, source_fingerprint: vehicle.vehicle_fingerprint, colour });
    }
    return [...seen.values()].sort((a, b) => DEFAULT_PAINT.variantKey(a.profile_id, a.source_fingerprint, a.colour)
      .localeCompare(DEFAULT_PAINT.variantKey(b.profile_id, b.source_fingerprint, b.colour)));
  },
});
