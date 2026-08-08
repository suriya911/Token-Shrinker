import { isSessionActive } from "../auth/session.mjs";

export function authorize(request, nowMs) {
  return isSessionActive(request.session.expiresAtMs, nowMs);
}
