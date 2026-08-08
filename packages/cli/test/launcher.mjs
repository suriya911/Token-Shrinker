import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { platformPackage, resolveBinary, verifyBinaryChecksum } from "../dist/index.js";

test("platform mapping is explicit and unsupported targets fail", () => {
  assert.equal(platformPackage("win32", "x64"), "@token-shrinker/cli-win32-x64");
  assert.equal(platformPackage("linux", "x64"), "@token-shrinker/cli-linux-x64-gnu");
  assert.throws(() => platformPackage("freebsd", "riscv64"), /no native package/);
});

test("environment override and checksum validation are deterministic", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-shrinker-cli-"));
  try {
    const binary = join(directory, "binary");
    await writeFile(binary, "fixture");
    process.env.TOKEN_SHRINKER_BINARY = binary;
    assert.equal(resolveBinary(), binary);
    const digest = createHash("sha256").update("fixture").digest("hex");
    assert.equal(await verifyBinaryChecksum(binary, digest), true);
    assert.equal(await verifyBinaryChecksum(binary, "0".repeat(64)), false);
  } finally {
    delete process.env.TOKEN_SHRINKER_BINARY;
    await rm(directory, { recursive: true, force: true });
  }
});
