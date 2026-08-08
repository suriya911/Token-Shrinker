import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(repositoryRoot, "fixtures", "demo-manifest.yaml");
const manifest = await readFile(manifestPath, "utf8");
const hashesSection = manifest.split(/^hashes:\s*$/mu)[1];

assert.ok(hashesSection, "demo manifest must contain a hashes section");

const expectedHashes = new Map();
for (const line of hashesSection.split(/\r?\n/u)) {
  const match = /^  (.+): ([0-9a-f]{64})$/u.exec(line);
  if (match) {
    expectedHashes.set(match[1], match[2]);
  }
}

assert.equal(expectedHashes.size, 11, "demo manifest must pin all 11 fixture files");

for (const [relativePath, expectedHash] of expectedHashes) {
  const content = await readFile(path.join(repositoryRoot, relativePath));
  const actualHash = createHash("sha256").update(content).digest("hex");
  assert.equal(actualHash, expectedHash, `${relativePath} hash mismatch`);
}

assert.equal(
  expectedHashes.get("fixtures/demo-repo/docs/session-policy.md"),
  expectedHashes.get("fixtures/demo-repo/docs/generated/session-policy-copy.md"),
  "duplicate documentation must remain byte-identical",
);

assert.match(
  manifest,
  /forbidden_context:[\s\S]*fixtures\/demo-repo\/secrets\/canary\.env/u,
  "secret canary must remain forbidden context",
);

const testRun = spawnSync(
  process.execPath,
  ["--test", "fixtures/demo-repo/tests/session.test.mjs"],
  { cwd: repositoryRoot, encoding: "utf8" },
);
const testOutput = `${testRun.stdout}\n${testRun.stderr}`;

assert.equal(testRun.status, 1, "demo test must fail before optimization work begins");
assert.match(testOutput, /session expiring now is rejected/u);
assert.match(testOutput, /true !== false/u);

console.log(`demo fixture verified (${expectedHashes.size} files, expected failure)`);
