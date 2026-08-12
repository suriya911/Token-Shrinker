import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const pluginRoot = resolve(root, "plugins", "token-shrinker");

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

const workspace = await readJson(resolve(root, "package.json"));
const marketplace = await readJson(resolve(root, ".agents", "plugins", "marketplace.json"));
const manifest = await readJson(resolve(pluginRoot, ".codex-plugin", "plugin.json"));
const mcp = await readJson(resolve(pluginRoot, ".mcp.json"));
const skill = await readFile(resolve(pluginRoot, "skills", "token-shrinker", "SKILL.md"), "utf8");

assert.equal(marketplace.name, "token-shrinker-plugins");
assert.equal(marketplace.plugins.length, 1);
assert.equal(marketplace.plugins[0].name, "token-shrinker");
assert.deepEqual(marketplace.plugins[0].source, {
  source: "local",
  path: "./plugins/token-shrinker",
});
assert.deepEqual(marketplace.plugins[0].policy, {
  installation: "AVAILABLE",
  authentication: "ON_INSTALL",
});

assert.equal(manifest.name, "token-shrinker");
assert.equal(manifest.version, workspace.version);
assert.equal(manifest.skills, "./skills/");
assert.equal(manifest.mcpServers, "./.mcp.json");
assert.ok(Array.isArray(manifest.interface.defaultPrompt));
assert.ok(manifest.interface.defaultPrompt.length <= 3);
assert.ok(manifest.interface.defaultPrompt.every((prompt) => prompt.length <= 128));
await access(resolve(pluginRoot, manifest.interface.logo));
await access(resolve(pluginRoot, manifest.interface.composerIcon));

const server = mcp.mcpServers["token-shrinker"];
assert.equal(server.command, "npx");
assert.deepEqual(server.args, [
  "--yes",
  `@token-shrinker/cli@${workspace.version}`,
  "start",
  "--stdio",
]);
assert.match(skill, /token-shrinker-owned:v1/);
assert.doesNotMatch(JSON.stringify(mcp), /OPENAI_BASE_URL|ANTHROPIC_BASE_URL|API_KEY/);

console.log("Codex plugin marketplace, manifest, MCP launcher, assets, and skill verified");
