import assert from "node:assert/strict";
import { test } from "node:test";
import {
  authorizeWorkspaceOperation,
  renderStructuredResult,
  statusLabel,
} from "../dist/core.js";

test("untrusted workspaces expose status but block repository operations", () => {
  assert.deepEqual(authorizeWorkspaceOperation("status", false), { allowed: true });
  assert.deepEqual(authorizeWorkspaceOperation("build-context", false), {
    allowed: false,
    warningCode: "workspace-untrusted",
  });
  assert.deepEqual(authorizeWorkspaceOperation("stats", false), {
    allowed: false,
    warningCode: "workspace-untrusted",
  });
  assert.deepEqual(authorizeWorkspaceOperation("build-context", true), { allowed: true });
});

test("structured output remains JSON data and health labels are bounded", () => {
  const value = { data: { health: "healthy", provider: "native-repository" } };
  assert.equal(statusLabel(value), "Token-Shrinker: healthy");
  assert.equal(statusLabel({ data: {} }), "Token-Shrinker: unavailable");
  assert.equal(statusLabel({ data: { health: "x".repeat(10_000) } }), "Token-Shrinker: unavailable");
  assert.equal(JSON.parse(renderStructuredResult(value)).data.provider, "native-repository");
});
