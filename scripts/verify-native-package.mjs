import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { access, chmod, copyFile, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import process from "node:process";

const targets = {
  "darwin-arm64": ["@token-shrinker/cli-darwin-arm64", "token-shrinker"],
  "darwin-x64": ["@token-shrinker/cli-darwin-x64", "token-shrinker"],
  "linux-x64": ["@token-shrinker/cli-linux-x64-gnu", "token-shrinker"],
  "win32-x64": ["@token-shrinker/cli-win32-x64", "token-shrinker.exe"],
};
const key = process.platform + "-" + process.arch;
const target = targets[key];
assert(target, "current platform is not an advertised native target: " + key);
const [packageName, executableName] = target;
const profileArgument = process.argv.indexOf("--profile");
const profile = profileArgument >= 0
  ? process.argv[profileArgument + 1]
  : process.env.TOKEN_SHRINKER_PROFILE ?? "debug";
assert(profile === "debug" || profile === "release", "profile must be debug or release");
const binary = process.env.TOKEN_SHRINKER_BINARY ??
  resolve("target", profile, process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");
await access(binary);
const versionRun = spawnSync(binary, ["version", "--json"], { encoding: "utf8" });
assert.equal(versionRun.status, 0, versionRun.stderr);
const version = JSON.parse(versionRun.stdout);
const cliManifest = JSON.parse(await readFile(resolve("packages", "cli", "package.json"), "utf8"));
const releaseVersion = cliManifest.version;
assert.equal(version.binaryVersion, releaseVersion);
assert.equal(version.packageVersion, releaseVersion);
assert.equal(version.protocolVersion, "1.0");
assert.equal(version.schemaVersion, 1);

function npmRun(args, cwd) {
  return process.platform === "win32"
    ? spawnSync(process.env.ComSpec ?? "cmd.exe",
        ["/d", "/s", "/c", "npm " + args.join(" ")], { cwd, encoding: "utf8" })
    : spawnSync("npm", args, { cwd, encoding: "utf8" });
}

const directory = await mkdtemp(join(tmpdir(), "token-shrinker-pack-"));
try {
  const staging = join(directory, "package");
  const output = join(directory, "out");
  await mkdir(join(staging, "bin"), { recursive: true });
  await mkdir(output);
  await copyFile(binary, join(staging, "bin", executableName));
  await copyFile(resolve("LICENSE"), join(staging, "LICENSE"));
  if (process.platform !== "win32") await chmod(join(staging, "bin", executableName), 0o755);
  await writeFile(join(staging, "package.json"), JSON.stringify({
    name: packageName, version: releaseVersion, license: "Apache-2.0",
    os: [process.platform], cpu: [process.arch], files: ["bin", "LICENSE"],
  }, null, 2));
  const packed = npmRun(["pack", "--json", "--pack-destination", output], staging);
  assert.equal(packed.status, 0, packed.stderr);
  const tarball = join(output, JSON.parse(packed.stdout)[0].filename);
  const listed = spawnSync("tar", ["-tzf", tarball], { encoding: "utf8" });
  assert.equal(listed.status, 0, listed.stderr);
  const files = listed.stdout.trim().split(/\r?\n/).sort();
  assert.deepEqual(files, ["package/LICENSE", "package/bin/" + executableName,
    "package/package.json"].sort());
  assert((await readFile(tarball)).byteLength > 0);

  const umbrella = join(directory, "umbrella");
  await mkdir(join(umbrella, "dist"), { recursive: true });
  for (const file of ["index.js", "index.js.map", "index.d.ts", "index.d.ts.map"]) {
    await copyFile(join("packages", "cli", "dist", file), join(umbrella, "dist", file));
  }
  await copyFile(join("packages", "cli", "dist", "LICENSE"), join(umbrella, "dist", "LICENSE"));
  await copyFile(join("packages", "cli", "README.md"), join(umbrella, "README.md"));
  await writeFile(join(umbrella, "package.json"), JSON.stringify({
    name: "@token-shrinker/cli", version: releaseVersion, type: "module",
    license: "Apache-2.0", files: ["dist", "README.md"],
    bin: { "token-shrinker": "./dist/index.js" },
    optionalDependencies: { [packageName]: releaseVersion },
  }, null, 2));
  const umbrellaPacked = npmRun(["pack", "--json", "--pack-destination", output], umbrella);
  assert.equal(umbrellaPacked.status, 0, umbrellaPacked.stderr);
  const umbrellaTarball = join(output, JSON.parse(umbrellaPacked.stdout)[0].filename);
  const umbrellaList = spawnSync("tar", ["-tzf", umbrellaTarball], { encoding: "utf8" });
  assert.equal(umbrellaList.status, 0, umbrellaList.stderr);
  assert.deepEqual(umbrellaList.stdout.trim().split(/\r?\n/).sort(), [
    "package/README.md", "package/dist/index.d.ts", "package/dist/index.d.ts.map",
    "package/dist/index.js", "package/dist/index.js.map", "package/dist/LICENSE",
    "package/package.json",
  ].sort());

  const installation = join(directory, "install");
  await mkdir(installation);
  await writeFile(join(installation, "package.json"), "{\"private\":true}");
  const installed = npmRun(["install", "--offline", "--ignore-scripts", tarball, umbrellaTarball], installation);
  assert.equal(installed.status, 0, installed.stderr);
  const shim = join(installation, "node_modules", ".bin",
    process.platform === "win32" ? "token-shrinker.cmd" : "token-shrinker");
  const smoke = process.platform === "win32"
    ? spawnSync(process.env.ComSpec ?? "cmd.exe",
        ["/d", "/s", "/c", shim + " version --json"], { encoding: "utf8" })
    : spawnSync(shim, ["version", "--json"], { encoding: "utf8" });
  assert.equal(smoke.status, 0, smoke.stderr);
  assert.equal(JSON.parse(smoke.stdout).protocolVersion, "1.0");
  const agentRoot = join(directory, "agent");
  const launcher = join(installation, "node_modules", "@token-shrinker", "cli", "dist", "index.js");
  await mkdir(agentRoot);
  const agents = [
    ["codex", [".codex", "config.toml"], [".agents", "skills", "token-shrinker", "SKILL.md"]],
    ["claude-code", [".mcp.json"], [".claude", "skills", "token-shrinker", "SKILL.md"]],
  ];
  for (const [agent, configPath, skillPath] of agents) {
    const add = spawnSync(process.execPath,
      [launcher, "add", agent, "--root", agentRoot, "--json"], { encoding: "utf8" });
    assert.equal(add.status, 0, add.stderr);
    const adapterResult = JSON.parse(add.stdout);
    assert.equal(adapterResult.serverProtocolValidated, true);
    assert.equal(adapterResult.clientApproval, "unknown");
    assert.equal(adapterResult.clientConnection, "not-checked");
    await access(join(agentRoot, ...configPath));
    await access(join(agentRoot, ...skillPath));
    const remove = spawnSync(process.execPath,
      [launcher, "remove", agent, "--root", agentRoot, "--json"], { encoding: "utf8" });
    assert.equal(remove.status, 0, remove.stderr);
  }
  const uninstalled = npmRun(["uninstall", "@token-shrinker/cli", packageName], installation);
  assert.equal(uninstalled.status, 0, uninstalled.stderr);
  await assert.rejects(access(shim));
  console.log("native package and Codex/Claude install smoke verified:", basename(tarball));
} finally {
  await rm(directory, { recursive: true, force: true });
}
