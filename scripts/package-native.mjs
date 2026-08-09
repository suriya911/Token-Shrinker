import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, copyFile, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";

const targets = {
  "darwin-arm64": ["packages/cli-darwin-arm64/package.json", "token-shrinker"],
  "darwin-x64": ["packages/cli-darwin-x64/package.json", "token-shrinker"],
  "linux-x64": ["packages/cli-linux-x64-gnu/package.json", "token-shrinker"],
  "win32-x64": ["packages/cli-win32-x64/package.json", "token-shrinker.exe"],
};
const target = targets[`${process.platform}-${process.arch}`];
assert(target, `unsupported release target: ${process.platform}-${process.arch}`);
const [manifestPath, executableName] = target;
const value = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
};
const output = resolve(value("--output", "release"));
const profile = value("--profile", "release");
assert(profile === "debug" || profile === "release", "profile must be debug or release");
const binary = resolve(value("--binary", join("target", profile, executableName)));
const manifest = JSON.parse(await readFile(resolve(manifestPath), "utf8"));
const rootVersion = JSON.parse(await readFile(resolve("package.json"), "utf8")).version;
assert.equal(manifest.version, rootVersion, "native package version must match the workspace");

const temporary = await mkdtemp(join(tmpdir(), "token-shrinker-native-stage-"));
try {
  await mkdir(join(temporary, "bin"), { recursive: true });
  await mkdir(output, { recursive: true });
  await copyFile(binary, join(temporary, "bin", executableName));
  await copyFile(resolve("LICENSE"), join(temporary, "LICENSE"));
  await writeFile(join(temporary, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
  if (process.platform !== "win32") await chmod(join(temporary, "bin", executableName), 0o755);
  const command = process.platform === "win32" ? process.execPath : "npm";
  const args = process.platform === "win32"
    ? [join(dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
        "pack", "--json", "--pack-destination", output]
    : ["pack", "--json", "--pack-destination", output];
  const packed = spawnSync(command, args, { cwd: temporary, encoding: "utf8" });
  assert.equal(packed.status, 0, packed.stderr);
  const result = JSON.parse(packed.stdout)[0];
  console.log(JSON.stringify({
    package: manifest.name,
    version: manifest.version,
    tarball: resolve(output, result.filename),
    integrity: result.integrity,
  }));
} finally {
  await rm(temporary, { recursive: true, force: true });
}
