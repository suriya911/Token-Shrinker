import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

function digest(content) {
  return createHash("sha256").update(content).digest("hex");
}

test("release artifact verifier checks every platform checksum set", async () => {
  const root = await mkdtemp(join(tmpdir(), "token-shrinker-rc-"));
  try {
    for (const target of ["windows", "linux", "macos-x64", "macos-arm64"]) {
      const directory = join(root, `token-shrinker-${target}`);
      await mkdir(directory, { recursive: true });
      const content = `artifact-${target}`;
      const line = `${digest(content)} *artifact.tgz\n`;
      await writeFile(join(directory, "artifact.tgz"), content);
      await writeFile(
        join(directory, "SHA256SUMS.txt"),
        line,
      );
      await writeFile(join(directory, "artifact.tgz.sha256"), line);
    }

    const script = join(process.cwd(), "scripts", "verify-rc-artifacts.mjs");
    const valid = spawnSync(process.execPath, [script, root], { encoding: "utf8" });
    assert.equal(valid.status, 0, valid.stderr);

    await writeFile(join(root, "token-shrinker-linux", "artifact.tgz"), "tampered");
    const invalid = spawnSync(process.execPath, [script, root], { encoding: "utf8" });
    assert.notEqual(invalid.status, 0);
    assert.match(invalid.stderr, /checksum mismatch/u);

    await writeFile(join(root, "token-shrinker-linux", "artifact.tgz"), "artifact-linux");
    await writeFile(
      join(root, "token-shrinker-linux", "artifact.tgz.sha256"),
      `${digest("wrong")} *artifact.tgz\n`,
    );
    const badSidecar = spawnSync(process.execPath, [script, root], { encoding: "utf8" });
    assert.notEqual(badSidecar.status, 0);
    assert.match(badSidecar.stderr, /sidecar mismatch/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
