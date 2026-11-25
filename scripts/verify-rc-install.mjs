import assert from "node:assert/strict";
import { access, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const artifactRoot = resolve(process.argv[2] ?? "release");
const tarballs = (await readdir(artifactRoot)).filter((name) => name.endsWith(".tgz"));

const readPackageName = (name) => name.replace(/-\d+\.\d+\.\d+\.tgz$/u, "");
const umbrella = tarballs.find((name) => readPackageName(name) === "token-shrinker-cli");
const native = tarballs.find((name) => readPackageName(name).startsWith("token-shrinker-cli-"));
assert(tarballs.length >= 2, `expected CLI and native tarballs in ${artifactRoot}`);
assert(umbrella, "CLI tarball is missing");
assert(native, "native tarball is missing");

const installRoot = await mkdtemp(join(tmpdir(), "token-shrinker-rc-install-"));
const commandOutput = (result) => result.stderr || result.stdout || "command failed without output";
const cmdArg = (arg) => /[\s"]/.test(arg) ? `"${arg.replaceAll('"', '\\"')}"` : arg;
const npmRun = (args, cwd) => process.platform === "win32"
  ? spawnSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", `npm ${args.map(cmdArg).join(" ")}`], {
      cwd, encoding: "utf8",
    })
  : spawnSync("npm", args, { cwd, encoding: "utf8" });
try {
  await writeFile(join(installRoot, "package.json"), JSON.stringify({ private: true }));
  const npm = npmRun([
    "install", "--ignore-scripts", "--no-audit", "--no-fund",
    join(artifactRoot, umbrella), join(artifactRoot, native),
  ], installRoot);
  assert.equal(npm.status, 0, commandOutput(npm));

  const cli = join(installRoot, "node_modules", "@token-shrinker", "cli", "dist", "index.js");
  const version = spawnSync(process.execPath, [cli, "version", "--json"], { encoding: "utf8" });
  assert.equal(version.status, 0, commandOutput(version));
  const parsed = JSON.parse(version.stdout);
  assert.equal(parsed.protocolVersion, "1.0");
  assert.equal(parsed.schemaVersion, 1);

  const agentRoot = join(installRoot, "agent");
  const add = spawnSync(process.execPath, [cli, "add", "codex", "--root", agentRoot, "--json"], {
    encoding: "utf8",
  });
  assert.equal(add.status, 0, commandOutput(add));
  assert.equal(JSON.parse(add.stdout).serverProtocolValidated, true);
  await access(join(agentRoot, ".codex", "config.toml"));

  const uninstall = npmRun([
    "uninstall", "@token-shrinker/cli", readPackageName(native), "--no-audit", "--no-fund",
  ], installRoot);
  assert.equal(uninstall.status, 0, commandOutput(uninstall));
  await assert.rejects(access(cli));
  console.log(`clean RC install verified: ${umbrella}, ${native}`);
} finally {
  await rm(installRoot, { recursive: true, force: true });
}
