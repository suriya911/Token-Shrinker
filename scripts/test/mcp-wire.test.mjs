import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const fixture = JSON.parse(readFileSync("fixtures/mcp/wire-v1.json", "utf8"));

/**
 * These assert properties of the frozen contract itself. Whether the running server still matches
 * the fixture is checked by `pnpm check:mcp-wire`, which needs a built binary; these run anywhere.
 */

test("the frozen contract declares the negotiated protocol version and server descriptor", () => {
  assert.equal(fixture.fixtureVersion, 1);
  assert.equal(fixture.initialize.protocolVersion, "2025-06-18");
  assert.equal(fixture.initialize.serverInfo.name, "token-shrinker");
  assert.equal(fixture.initialize.serverInfo.version, "<server-version>",
    "the server version must stay a placeholder so a release does not look like protocol drift");
});

test("every tool carries the name, input schema, and output schema a client compiles against", () => {
  assert.ok(fixture.tools.length > 0, "the contract must describe at least one tool");
  for (const tool of fixture.tools) {
    assert.match(tool.name, /^token_shrinker_[a-z_]+$/, `unexpected tool name ${tool.name}`);
    assert.equal(typeof tool.description, "string");
    assert.ok(tool.description.length > 0, `${tool.name} has no description`);
    assert.equal(tool.inputSchema?.type, "object", `${tool.name} has no object input schema`);
    assert.equal(tool.outputSchema?.type, "object", `${tool.name} has no object output schema`);
  }
});

test("every response envelope keeps the four fields the reference documents as required", () => {
  for (const tool of fixture.tools) {
    assert.deepEqual([...tool.outputSchema.required].sort(),
      ["data", "protocolVersion", "requestId", "warnings"].sort(),
      `${tool.name} changed its envelope contract`);
  }
});

test("tool names are unique and stored in a stable order", () => {
  const names = fixture.tools.map((tool) => tool.name);
  assert.equal(new Set(names).size, names.length, "duplicate tool names in the contract");
  assert.deepEqual(names, [...names].sort(),
    "tools must stay sorted so a regenerated fixture produces a reviewable diff");
});

test("input schemas are closed so an unknown field is rejected rather than ignored", () => {
  for (const tool of fixture.tools) {
    assert.equal(tool.inputSchema.additionalProperties, false,
      `${tool.name} accepts unknown input fields`);
  }
});

test("the tools that mutate state or run processes are not annotated read-only", () => {
  const mutating = ["token_shrinker_execute", "token_shrinker_remember",
    "token_shrinker_task_update", "token_shrinker_record_tokens"];
  for (const name of mutating) {
    const tool = fixture.tools.find((candidate) => candidate.name === name);
    assert.ok(tool, `${name} is missing from the contract`);
    assert.notEqual(tool.annotations?.readOnlyHint, true,
      `${name} mutates state but advertises readOnlyHint, which misleads client approval prompts`);
  }
});

test("the documented read-only tools stay annotated read-only", () => {
  const readOnly = ["token_shrinker_capabilities", "token_shrinker_route",
    "token_shrinker_build_context", "token_shrinker_fetch_source",
    "token_shrinker_search_memory", "token_shrinker_stats",
    "token_shrinker_format_final", "token_shrinker_task_status"];
  for (const name of readOnly) {
    const tool = fixture.tools.find((candidate) => candidate.name === name);
    assert.ok(tool, `${name} is missing from the contract`);
    assert.equal(tool.annotations?.readOnlyHint, true, `${name} lost its read-only annotation`);
  }
});
