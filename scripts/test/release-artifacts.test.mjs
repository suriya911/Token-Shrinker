import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
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
      await writeFile(join(directory, "artifact.tgz"), content);
      const files = ["artifact.tgz"];
      if (target === "linux") {
        await writeFile(join(directory, "token-shrinker.cdx.json"), JSON.stringify({
          bomFormat: "CycloneDX",
          specVersion: "1.6",
          serialNumber: "urn:uuid:00000000-0000-4000-8000-000000000001",
          version: 1,
          components: [{ type: "library", name: "fixture", version: "1.0.0" }],
        }));
        files.push("token-shrinker.cdx.json");
      }
      const lines = [];
      for (const file of files) {
        const line = `${digest(await readFile(join(directory, file)))} *${file}\n`;
        lines.push(line);
        await writeFile(join(directory, `${file}.sha256`), line);
      }
      await writeFile(join(directory, "SHA256SUMS.txt"), lines.join(""));
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

    await writeFile(
      join(root, "token-shrinker-linux", "artifact.tgz.sha256"),
      `${digest("artifact-linux")} *artifact.tgz\n`,
    );
    const badSbomContent = JSON.stringify({
      bomFormat: "Not-CycloneDX",
      specVersion: "1.6",
      components: [{ type: "library", name: "fixture", version: "1.0.0" }],
    });
    const badSbomLine = `${digest(badSbomContent)} *token-shrinker.cdx.json\n`;
    await writeFile(join(root, "token-shrinker-linux", "token-shrinker.cdx.json"), badSbomContent);
    await writeFile(join(root, "token-shrinker-linux", "token-shrinker.cdx.json.sha256"), badSbomLine);
    const checksumPath = join(root, "token-shrinker-linux", "SHA256SUMS.txt");
    const checksumText = await readFile(checksumPath, "utf8");
    await writeFile(checksumPath, checksumText.replace(/^[0-9a-f]{64} \*token-shrinker\.cdx\.json$/mu, badSbomLine.trim()));
    const badSbom = spawnSync(process.execPath, [script, root], { encoding: "utf8" });
    assert.notEqual(badSbom.status, 0);
    assert.match(badSbom.stderr, /not CycloneDX/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
