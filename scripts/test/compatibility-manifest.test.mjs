import assert from "node:assert/strict";
import { createPrivateKey, createPublicKey } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const seed = Buffer.alloc(32, 7);
const privateKey = createPrivateKey({
  key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]),
  format: "der",
  type: "pkcs8",
});
const publicKey = createPublicKey(privateKey).export({ format: "der", type: "spki" }).subarray(-32).toString("hex");

test("compatibility manifest signing matches independent verification", async () => {
  const root = await mkdtemp(join(tmpdir(), "token-shrinker-manifest-"));
  try {
    const input = join(root, "unsigned.json");
    const output = join(root, "signed.json");
    await writeFile(input, JSON.stringify({
      schemaVersion: 1,
      generatedAt: "2026-11-16T12:00:00Z",
      expiresAt: "2026-11-23T12:00:00Z",
      components: [{
        id: "token-shrinker",
        authoritativeSource: "https://github.com/suriya911/Token-Shrinker",
        ownership: "cargo",
        releases: [{
          version: "0.2.2",
          channel: "beta",
          protocol: { min: "1.0.0", max: "1.0.0" },
          artifacts: [{ platform: "windows-x64", url: "https://example.com/token-shrinker.zip", sha256: "a".repeat(64) }],
        }],
      }],
    }));
    const env = { ...process.env, TOKEN_SHRINKER_ED25519_PRIVATE_KEY: seed.toString("hex"), TOKEN_SHRINKER_SIGNING_KEY_ID: "test-key" };
    const signed = spawnSync(process.execPath, ["scripts/sign-compatibility-manifest.mjs", input, output], { encoding: "utf8", env });
    assert.equal(signed.status, 0, signed.stderr);
    const verified = spawnSync(process.execPath, ["scripts/verify-compatibility-manifest.mjs", output], {
      encoding: "utf8",
      env: { ...process.env, TOKEN_SHRINKER_ED25519_PUBLIC_KEY: publicKey },
    });
    assert.equal(verified.status, 0, verified.stderr);
    const tampered = JSON.parse(await readFile(output, "utf8"));
    tampered.components[0].releases[0].version = "0.2.3";
    await writeFile(output, JSON.stringify(tampered));
    const rejected = spawnSync(process.execPath, ["scripts/verify-compatibility-manifest.mjs", output], {
      encoding: "utf8",
      env: { ...process.env, TOKEN_SHRINKER_ED25519_PUBLIC_KEY: publicKey },
    });
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /signature verification failed/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
