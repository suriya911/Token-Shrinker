import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const jsonPath = process.argv[2] ?? "ci-benchmark/public-demo.json";
const markdownPath = jsonPath.replace(/\.json$/u, ".md");
const fail = (message) => {
  throw new Error(`Benchmark artifact verification failed: ${message}`);
};

if (!existsSync(jsonPath)) fail(`missing ${jsonPath}`);
if (!existsSync(markdownPath)) fail(`missing ${markdownPath}`);

const report = JSON.parse(readFileSync(jsonPath, "utf8"));
if (report.schemaVersion !== 2) fail("report schema must be v2");
if (report.passed !== true) fail("report did not pass acceptance thresholds");
if (report.providers?.optionalProviders !== false) fail("optional providers were enabled");
if (report.reductionBasisPoints < 3000) fail("reduction is below 30%");
if (report.requiredEvidenceRecallBasisPoints < 9500) fail("recall is below 95%");

const reproducibility = report.reproducibility;
if (!reproducibility || typeof reproducibility !== "object") {
  fail("reproducibility metadata is missing");
}
for (const field of [
  "gitCommit",
  "gitDirty",
  "cpu",
  "memoryBytes",
  "filesystem",
  "powerMode",
  "rustVersion",
  "nodeVersion",
  "warmupCount",
  "sampleCount",
  "rawSamples",
  "cacheState",
]) {
  if (!(field in reproducibility)) fail(`missing reproducibility field ${field}`);
}
if (!Number.isInteger(reproducibility.sampleCount) || reproducibility.sampleCount < 1) {
  fail("sampleCount must be a positive integer");
}
for (const [name, samples] of Object.entries(reproducibility.rawSamples ?? {})) {
  if (!Array.isArray(samples) || samples.length !== reproducibility.sampleCount) {
    fail(`raw sample series ${name} does not match sampleCount`);
  }
}

const workspaceVersion = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "Cargo.toml"), "utf8")
  .match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
if (report.environment?.binaryVersion !== workspaceVersion) {
  fail(`report version ${report.environment?.binaryVersion} does not match workspace ${workspaceVersion}`);
}
const markdown = readFileSync(markdownPath, "utf8");
if (!markdown.includes(`version ${workspaceVersion}`)) fail("Markdown lacks report version");
if (!markdown.includes("Raw data:")) fail("Markdown lacks raw-data link");

console.log(`Benchmark artifacts verified: ${jsonPath} and ${markdownPath}`);
