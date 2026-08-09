import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const fail = (message) => {
  throw new Error(`M7 verification failed: ${message}`);
};

const requiredFiles = [
  "docs/threat-model.md",
  "deny.toml",
  ".github/workflows/security.yml",
  "fuzz/Cargo.toml",
  "benchmarks/public-demo.json",
  "benchmarks/public-demo.md",
];
for (const file of requiredFiles) {
  if (!existsSync(file)) fail(`missing ${file}`);
}

const workflow = readFileSync(".github/workflows/security.yml", "utf8");
for (const match of workflow.matchAll(/^\s*uses:\s*([^\s#]+)/gm)) {
  const value = match[1];
  if (!/@[0-9a-f]{40}$/.test(value)) fail(`unpinned action ${value}`);
}

const threatModel = readFileSync("docs/threat-model.md", "utf8");
for (const section of ["Trust boundaries", "Threats, mitigations, and verification", "Residual risk", "Review status"]) {
  if (!threatModel.includes(section)) fail(`threat model lacks ${section}`);
}

const report = JSON.parse(readFileSync("benchmarks/public-demo.json", "utf8"));
if (report.passed !== true) fail("public demo did not pass");
if (report.reductionBasisPoints < 3000) fail("context reduction is below 30%");
if (report.requiredEvidenceRecallBasisPoints < 9500) fail("evidence recall is below 95%");
if (report.citationCorrectnessBasisPoints !== 10000) fail("citations are not 100% correct");
if (report.providers.optionalProviders !== false) fail("optional provider must be disabled");

const vsix = join("packages", "vscode", "dist", "token-shrinker.vsix");
if (!existsSync(vsix)) fail("VSIX was not created");
const archiveCommand = process.platform === "win32" ? "tar" : "unzip";
const listArgs = process.platform === "win32" ? ["-tf", vsix] : ["-Z1", vsix];
const listing = execFileSync(archiveCommand, listArgs, { encoding: "utf8" }).replaceAll("\\", "/");
for (const entry of ["extension.vsixmanifest", "extension/package.json", "extension/dist/index.js"]) {
  if (!listing.includes(entry)) fail(`VSIX lacks ${entry}`);
}
for (const forbidden of ["extension/src/", "extension/test/", "extension/node_modules/"]) {
  if (listing.includes(forbidden)) fail(`VSIX contains ${forbidden}`);
}

const manifest = JSON.parse(
  execFileSync(
    archiveCommand,
    process.platform === "win32"
      ? ["-xOf", vsix, "extension/package.json"]
      : ["-p", vsix, "extension/package.json"],
    { encoding: "utf8" },
  ),
);
if (manifest.capabilities?.untrustedWorkspaces?.supported !== "limited") {
  fail("VSIX does not declare limited untrusted-workspace support");
}
if (manifest.contributes?.commands?.length !== 3) fail("VSIX command contract changed");

console.log("M7 static review passed: pinned actions, threat model, benchmark, and minimal VSIX.");
