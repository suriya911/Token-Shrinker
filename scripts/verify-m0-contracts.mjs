import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const referenceTime = Date.parse("2026-08-07T12:00:00Z");
const draft202012 = "https://json-schema.org/draft/2020-12/schema";

async function readJson(path) {
  return JSON.parse(await readFile(new URL(`../${path}`, import.meta.url), "utf8"));
}

const [compatibilitySchema, toolSchema, publicSchema, valid, expired, incompatible, tampered, transport, fixtureKey] =
  await Promise.all([
    readJson("schemas/compatibility-manifest.schema.json"),
    readJson("schemas/tool-descriptor.schema.json"),
    readJson("schemas/public-envelope.schema.json"),
    readJson("fixtures/compatibility/valid-manifest.json"),
    readJson("fixtures/compatibility/expired-manifest.json"),
    readJson("fixtures/compatibility/incompatible-manifest.json"),
    readJson("fixtures/compatibility/tampered-artifact.json"),
    readJson("fixtures/native-transport/claude-code.json"),
    readFile(new URL("../fixtures/compatibility/fixture-key-1.pub", import.meta.url), "utf8"),
  ]);

for (const schema of [compatibilitySchema, toolSchema, publicSchema]) {
  assert.equal(schema.$schema, draft202012);
  assert.equal(schema.type, "object");
  assert.equal(schema.additionalProperties, false);
}

assert.equal(valid.schemaVersion, 1);
assert.ok(Date.parse(valid.expiresAt) > referenceTime);
assert.ok(valid.components.length > 0);
assert.equal(valid.signature.algorithm, "ed25519");
assert.equal(fixtureKey.trim().length, 64);
assert.ok(valid.signature.value.length >= 88);
assert.deepEqual(valid.components[0].releases[0].protocol, {
  min: "1.0.0",
  max: "1.0.0",
});

assert.ok(Date.parse(expired.expiresAt) < referenceTime);

const incompatibleProtocol = incompatible.components[0].releases[0].protocol;
assert.ok(Number(incompatibleProtocol.min.split(".")[0]) > 1);

assert.notEqual(tampered.declaredSha256, tampered.observedSha256);
assert.equal(tampered.expectedDecision, "reject");

assert.deepEqual(transport.after, transport.before);
assert.equal(transport.expected.transportUnchanged, true);
assert.equal(transport.expected.remoteControlEligible, true);
assert.ok(transport.forbiddenMutations.includes("ANTHROPIC_BASE_URL"));
assert.ok(transport.forbiddenMutations.includes("ANTHROPIC_API_KEY"));

console.log("Contracts verified (3 schemas, signed update fixture, 5 regression fixtures).");
