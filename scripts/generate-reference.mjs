import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import process from "node:process";

const binary = process.env.TOKEN_SHRINKER_BINARY ??
  resolve("target", "debug", process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");
const run = spawnSync(binary, ["reference", "--json"], { encoding: "utf8" });
assert.equal(run.status, 0, run.stderr);
const reference = JSON.parse(run.stdout);
const tick = String.fromCharCode(96);
const cli = "# CLI reference\n\nGenerated from the native command metadata. Do not edit manually.\n\n" +
  reference.commands.map((command) => "- " + tick + "token-shrinker " + command + tick).join("\n") + "\n";
const mcp = "# MCP tool reference\n\nGenerated from the Rust MCP tool metadata. Do not edit manually.\n\n" +
  reference.tools.map((tool) => "## " + tick + tool.name + tick + "\n\n" + tool.description +
    "\n\n- Read only: " + tick + tool.annotations.readOnlyHint + tick +
    "\n- Destructive: " + tick + tool.annotations.destructiveHint + tick +
    "\n- Idempotent: " + tick + tool.annotations.idempotentHint + tick + "\n").join("\n");
const outputs = [["docs/reference/cli.md", cli], ["docs/reference/mcp.md", mcp]];
for (const [path, content] of outputs) {
  if (process.argv.includes("--check")) {
    assert.equal(await readFile(path, "utf8"), content, path + " is stale");
  } else {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
}
