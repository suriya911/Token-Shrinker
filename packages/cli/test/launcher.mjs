import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { adapterCommand, platformPackage, resolveBinary, verifyBinaryChecksum } from
  "../dist/index.js";

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

test("agent adapters support preview, atomic apply, and owned removal", async () => {
  const root = await mkdtemp(join(tmpdir(), "token-shrinker-agent-install-"));
  try {
    const preview = await adapterCommand(["add", "codex", "--root", root, "--dry-run"],
      { binaryPath: "token-shrinker", validate: false });
    assert.equal(preview?.applied, false);
    assert.equal(preview?.nativeTransportUnchanged, true);
    await assert.rejects(access(join(root, ".codex", "config.toml")));

    const binaryPath = join(root, "bin", "token-shrinker");
    const installed = await adapterCommand(["add", "codex", "--root", root],
      { binaryPath, validate: false });
    assert.equal(installed?.applied, true);
    const config = await readFile(join(root, ".codex", "config.toml"), "utf8");
    assert.match(config, /mcp_servers\.token-shrinker/);
    assert.ok(config.includes(binaryPath.replaceAll("\\", "\\\\")));
    assert.match(await readFile(join(root, ".agents", "skills", "token-shrinker", "SKILL.md"),
      "utf8"), /token_shrinker_build_context/);

    const removed = await adapterCommand(["remove", "codex", "--root", root],
      { binaryPath, validate: false });
    assert.equal(removed?.applied, true);
    await assert.rejects(access(join(root, ".agents", "skills", "token-shrinker", "SKILL.md")));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
