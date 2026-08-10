import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import process from "node:process";

const manifests = [
  "package.json",
  "packages/adapter-aider/package.json",
  "packages/adapter-claude-code/package.json",
  "packages/adapter-codex/package.json",
  "packages/adapter-core/package.json",
  "packages/adapter-gemini/package.json",
  "packages/adapter-opencode/package.json",
  "packages/cli/package.json",
  "packages/sdk/package.json",
  "packages/vscode/package.json",
  "packages/cli-win32-x64/package.json",
  "packages/cli-linux-x64-gnu/package.json",
  "packages/cli-darwin-arm64/package.json",
  "packages/cli-darwin-x64/package.json",
];
const loaded = await Promise.all(manifests.map(async (path) => [
  path,
  JSON.parse(await readFile(resolve(path), "utf8")),
]));
const releaseVersion = loaded[0][1].version;
assert.match(releaseVersion, /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/,
  "workspace version must be valid SemVer");
for (const [path, manifest] of loaded) {
  assert.equal(manifest.version, releaseVersion, `${path} version is not coordinated`);
  assert.equal(manifest.license, "Apache-2.0", `${path} must declare Apache-2.0`);
}

const cargo = await readFile(resolve("Cargo.toml"), "utf8");
assert.match(cargo, new RegExp(`\\[workspace\\.package\\][\\s\\S]*?version = "${
  releaseVersion.replaceAll(".", "\\.")}"`), "Cargo workspace version is not coordinated");

const sdkSource = await readFile(resolve("packages/sdk/src/index.ts"), "utf8");
assert.match(sdkSource, new RegExp(`clientInfo: \\{ name: "@token-shrinker/sdk", version: "${
  releaseVersion.replaceAll(".", "\\.")}" \\}`),
"SDK MCP client version is not coordinated");

const cli = loaded.find(([path]) => path === "packages/cli/package.json")[1];
assert.equal(cli.publishConfig?.access, "public");
assert(cli.files.includes("dist/LICENSE"), "CLI tarball must contain the license");
assert.equal(cli.repository?.url, "https://github.com/suriya911/Token-Shrinker.git");

const sdk = loaded.find(([path]) => path === "packages/sdk/package.json")[1];
assert.equal(sdk.publishConfig?.access, "public");
assert(sdk.files.includes("dist/LICENSE"), "SDK tarball must contain the license");

for (const [path, manifest] of loaded.filter(([path]) => path.includes("packages/cli-"))) {
  assert.equal(manifest.publishConfig?.access, "public", `${path} must publish publicly`);
  assert(manifest.files.includes("LICENSE"), `${path} tarball must contain the license`);
  assert.deepEqual(manifest.cpu.length, 1, `${path} must select one CPU`);
  assert.deepEqual(manifest.os.length, 1, `${path} must select one OS`);
}

const vscode = loaded.find(([path]) => path === "packages/vscode/package.json")[1];
assert.equal(vscode.publisher, "token-shrinker");
assert.match(vscode.engines.vscode, /^>=1\./);
assert(!vscode.dependencies?.["@token-shrinker/sdk"],
  "bundled VSIX must not retain a workspace runtime dependency");

if (process.argv.includes("--publish")) {
  assert.notEqual(releaseVersion, "0.0.0", "set an RC or stable version before publishing");
  assert(process.env.TOKEN_SHRINKER_RELEASE_OWNER_ACK === "yes",
    "publishing requires TOKEN_SHRINKER_RELEASE_OWNER_ACK=yes after registry ownership is verified");
}

console.log(`release metadata verified for ${releaseVersion}${
  releaseVersion === "0.0.0" ? " (development version; publishing disabled)" : ""}`);
