import assert from "node:assert/strict";
import test from "node:test";

import { authorize } from "../src/api/authorize.mjs";

test("session expiring now is rejected", () => {
  const nowMs = 1_700_000_000_000;
  const request = { session: { expiresAtMs: nowMs } };

  assert.equal(authorize(request, nowMs), false);
});
