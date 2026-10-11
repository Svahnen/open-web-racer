// Per-player gearbox mode shared by the solo and room set-up UI. No DOM here;
// unit tested. The wire value is the protocol `Transmission` enum in
// snake_case: "automatic" (default) or "manual".

export const TRANSMISSION_AUTOMATIC = "automatic";
export const TRANSMISSION_MANUAL = "manual";

export function clampTransmission(value) {
  return value === TRANSMISSION_MANUAL ? TRANSMISSION_MANUAL : TRANSMISSION_AUTOMATIC;
}

export function isManual(value) {
  return clampTransmission(value) === TRANSMISSION_MANUAL;
}

/** Solo setting from a query string (`gearbox=manual`); anything else is automatic. */
export function parseTransmissionParam(search) {
  return clampTransmission(new URLSearchParams(search).get("gearbox"));
}

/** Query fragment for the solo link; empty for the default so old URLs keep their meaning. */
export function transmissionQuery(value) {
  return isManual(value) ? `&gearbox=${TRANSMISSION_MANUAL}` : "";
}

export function transmissionLabel(value) {
  return isManual(value) ? "Manual" : "Automatic";
}

/** JSON form of the `SetTransmission` client message for `encode_client_message`. */
export function setTransmissionMessage(value, autoReverse = false) {
  return { set_transmission: { transmission: clampTransmission(value), auto_reverse: autoReverse === true } };
}

/** One-line summary for the set-up and lobby. */
export function transmissionSummary(value) {
  return isManual(value)
    ? "Manual gearbox · shift with A and Z through R, N, 1-6"
    : "Automatic gearbox · A and Z select drive, neutral and reverse";
}
