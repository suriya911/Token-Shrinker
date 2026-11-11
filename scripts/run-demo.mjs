/**
 * Runs the deterministic public demo and prints its JSON report.
 *
 * By default this builds and runs the workspace CLI from source, which is what CI must do so the
 * demo measures the code under review. Setting TOKEN_SHRINKER_BINARY points the run at an already
 * built executable instead, matching generate-protocol.mjs, generate-reference.mjs, and
 * verify-native-package.mjs. That path exists for local runs on machines without a working Rust
 * toolchain; CI never sets the variable.
 */
import { spawnSync } from "node:child_process";
import process from "node:process";

const binary = process.env.TOKEN_SHRINKER_BINARY;
const demoArgs = ["benchmark", "demo", "--json"];
const run = binary
  ? spawnSync(binary, demoArgs, { encoding: "utf8" })
  : spawnSync("cargo", ["run", "-q", "-p", "token-shrinker-cli", "--", ...demoArgs],
      { encoding: "utf8" });

if (run.error) {
  process.stderr.write(`failed to launch the demo: ${run.error.message}\n`);
  process.exit(1);
}
if (run.status !== 0) {
  process.stderr.write(run.stderr ?? "");
  process.exit(run.status ?? 1);
}

const report = JSON.parse(run.stdout);
if (report.passed !== true) {
  process.stderr.write("demo reported a failing run\n");
  process.stdout.write(JSON.stringify(report, null, 2) + "\n");
  process.exit(1);
}
process.stdout.write(JSON.stringify(report, null, 2) + "\n");
