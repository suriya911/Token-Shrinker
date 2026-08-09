import { execFileSync } from "node:child_process";

const pnpmScript = process.env.npm_execpath;
const executable = pnpmScript ? process.execPath : "pnpm";
const args = pnpmScript
  ? [pnpmScript, "licenses", "list", "--prod", "--json"]
  : ["licenses", "list", "--prod", "--json"];
const output = execFileSync(executable, args, {
  encoding: "utf8",
});
const trimmed = output.trim();
const inventory = trimmed.startsWith("{") || trimmed.startsWith("[") ? JSON.parse(trimmed) : {};
if (trimmed && !trimmed.startsWith("No licenses in packages found")) {
  throw new Error(`Unexpected pnpm license output: ${trimmed.slice(0, 120)}`);
}
const denied = Object.keys(inventory).filter((license) =>
  /(?:^|\W)(?:AGPL|GPL|LGPL|SSPL|UNKNOWN)(?:\W|$)/i.test(license),
);
if (denied.length > 0) {
  console.error(`Disallowed or unknown production licenses: ${denied.join(", ")}`);
  process.exitCode = 1;
} else {
  console.log(`Production Node license scan passed (${Object.keys(inventory).length} license groups).`);
}
