import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const tracked = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8" })
  .split("\0")
  .filter(Boolean);
const allowedCanaries = new Set(["fixtures/demo-repo/secrets/canary.env"]);
const rules = [
  ["private-key", new RegExp(["-----BEGIN ", "(?:RSA |EC |OPENSSH )?PRIVATE KEY-----"].join(""))],
  ["github-token", /\b(?:ghp_|github_pat_)[A-Za-z0-9_]{20,}\b/],
  ["openai-key", /\bsk-[A-Za-z0-9]{20,}\b/],
  ["aws-access-key", /\bAKIA[0-9A-Z]{16}\b/],
];
const findings = [];
for (const path of tracked) {
  if (allowedCanaries.has(path)) continue;
  let bytes;
  try {
    bytes = readFileSync(path);
  } catch {
    continue;
  }
  if (bytes.length > 2 * 1024 * 1024 || bytes.includes(0)) continue;
  const text = bytes.toString("utf8");
  for (const [rule, pattern] of rules) {
    if (pattern.test(text)) findings.push(`${path}: ${rule}`);
  }
}
if (findings.length > 0) {
  console.error(findings.join("\n"));
  process.exitCode = 1;
} else {
  console.log(`Secret scan passed (${tracked.length} tracked files, synthetic canary allowlisted).`);
}
