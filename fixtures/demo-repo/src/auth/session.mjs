/**
 * Return whether a session remains active at evaluation time.
 * Policy requires expiration to be strictly later than the current time.
 */
export function isSessionActive(expiresAtMs, nowMs) {
  return expiresAtMs >= nowMs;
}
