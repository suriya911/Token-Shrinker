/**
 * Captures the MCP wire contract into fixtures/mcp/wire-v1.json, or verifies it with --check.
 *
 * Protocol v1 is frozen, so the advertised tool surface must not drift silently. `tools/list`
 * carries every tool's name, annotations, input schema, and output schema, which is the whole
 * public request/response contract; `initialize` carries the negotiated protocol version and the
 * server descriptor. Both are recorded verbatim apart from the server version, which moves every
 * release and is checked by verify-release-metadata.mjs instead.
 *
 * A failure here is not a formatting nit. It means a client written against v1 may break.
 */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import process from "node:process";

const FIXTURE = resolve("fixtures", "mcp", "wire-v1.json");
const VERSION_PLACEHOLDER = "<server-version>";
const check = process.argv.includes("--check");

const binary = process.env.TOKEN_SHRINKER_BINARY
  ?? resolve("target", "debug", process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");

/** Sends newline-delimited JSON-RPC requests to one server process and collects the replies. */
function callServer(requests) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(binary, ["start", "--stdio"], { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", rejectPromise);
    child.on("close", (code) => {
      if (code !== 0 && stdout.trim() === "") {
        rejectPromise(new Error(`server exited with ${code}: ${stderr}`));
        return;
      }
      resolvePromise(stdout.trim().split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line)));
    });
    for (const request of requests) child.stdin.write(JSON.stringify(request) + "\n");
    child.stdin.end();
  });
}

const replies = await callServer([
  {
    jsonrpc: "2.0", id: 1, method: "initialize",
    params: {
      protocolVersion: "2025-06-18", capabilities: {},
      clientInfo: { name: "token-shrinker-wire-fixture", version: "1" },
    },
  },
  { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
]);

const byId = new Map(replies.map((reply) => [reply.id, reply]));
const initialize = byId.get(1);
const toolsList = byId.get(2);
assert(initialize?.result, "initialize did not return a result");
assert(toolsList?.result?.tools, "tools/list did not return tools");

const serverInfo = { ...initialize.result.serverInfo, version: VERSION_PLACEHOLDER };
const captured = {
  fixtureVersion: 1,
  note: "Frozen MCP wire contract for protocol v1. Regenerate only with a deliberate, "
    + "documented protocol change; see docs/reference/protocol-v1-compatibility.md.",
  initialize: { ...initialize.result, serverInfo },
  tools: [...toolsList.result.tools].sort((a, b) => a.name.localeCompare(b.name)),
};

const serialized = JSON.stringify(captured, null, 2) + "\n";

if (!check) {
  await mkdir(dirname(FIXTURE), { recursive: true });
  await writeFile(FIXTURE, serialized);
  console.log(`captured ${captured.tools.length} MCP tools into fixtures/mcp/wire-v1.json`);
} else {
  const expected = JSON.parse(await readFile(FIXTURE, "utf8"));
  const actualNames = captured.tools.map((tool) => tool.name);
  const expectedNames = expected.tools.map((tool) => tool.name);
  const added = actualNames.filter((name) => !expectedNames.includes(name));
  const removed = expectedNames.filter((name) => !actualNames.includes(name));
  assert.deepEqual(removed, [],
    `protocol v1 removed tools ${removed.join(", ")}; removing a tool requires protocol v2`);
  assert.deepEqual(added, [],
    `protocol v1 added tools ${added.join(", ")}; regenerate the fixture in the same change that `
    + "documents the addition");
  for (const tool of captured.tools) {
    const frozen = expected.tools.find((candidate) => candidate.name === tool.name);
    assert.deepEqual(tool, frozen,
      `the wire contract for ${tool.name} changed; a v1 client may break`);
  }
  assert.deepEqual(captured.initialize, expected.initialize,
    "the initialize result changed; a v1 client may break");
  console.log(`MCP wire contract matches the frozen v1 fixture (${captured.tools.length} tools).`);
}
