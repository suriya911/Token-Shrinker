import test from "node:test";
import assert from "node:assert/strict";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { openCodeAdapter } from "../dist/index.js";

test("OpenCode adapter uses the current mcp.servers shape", () => {
  const config = openCodeAdapter.config("token-shrinker");
  assert.deepEqual(config.jsonPath, ["mcp", "servers", "token-shrinker"]);
  assert.deepEqual(config.jsonValue, {
    type: "local",
    command: ["token-shrinker", "start", "--stdio"],
  });
});

test("OpenCode adapter lifecycle preserves native configuration", async () =>
  runAdapterContract(openCodeAdapter));
