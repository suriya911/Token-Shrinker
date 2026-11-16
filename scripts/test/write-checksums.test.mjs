import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

test("write-checksums emits aggregate and per-file sidecars", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-shrinker-checksums-"));
  try {
    const content = "release artifact";
    await writeFile(join(directory, "package.tgz"), content);
    const run = spawnSync(process.execPath, [
      join(process.cwd(), "scripts", "write-checksums.mjs"),
      directory,
    ], { encoding: "utf8" });
    assert.equal(run.status, 0, run.stderr);
    const digest = createHash("sha256").update(content).digest("hex");
    const expected = `${digest} *package.tgz\n`;
    assert.equal(await readFile(join(directory, "SHA256SUMS.txt"), "utf8"), expected);
    assert.equal(await readFile(join(directory, "package.tgz.sha256"), "utf8"), expected);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
