import assert from "node:assert/strict";
import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { codexAdapter } from "../dist/index.js";
test("Codex adapter lifecycle owns only its MCP fragment", async () =>
  runAdapterContract(codexAdapter));

test("Codex adapter gates the executing tool behind explicit approval", () => {
  const fragment = codexAdapter.config("/tmp/token-shrinker").content;
  assert.match(fragment, /\[mcp_servers\.token-shrinker\.tools\.token_shrinker_execute\]/,
    "the process-executing tool must declare a Codex approval policy");
  assert.match(fragment, /approval_mode = "approve"/,
    "token_shrinker_execute must require approval, never run unattended");
  const gateIndex = fragment.indexOf("[mcp_servers.token-shrinker.tools.token_shrinker_execute]");
  const serverIndex = fragment.indexOf("[mcp_servers.token-shrinker]");
  assert.ok(serverIndex >= 0 && gateIndex > serverIndex,
    "the tool table must follow its parent server table so the fragment parses as TOML");
});
