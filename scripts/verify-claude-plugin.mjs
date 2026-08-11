import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const pluginRoot = resolve(root, "plugins", "claude", "token-shrinker");

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

const workspace = await readJson(resolve(root, "package.json"));
const marketplace = await readJson(resolve(root, ".claude-plugin", "marketplace.json"));
const manifest = await readJson(resolve(pluginRoot, ".claude-plugin", "plugin.json"));
const mcp = await readJson(resolve(pluginRoot, ".mcp.json"));
const pluginPackage = await readJson(resolve(pluginRoot, "package.json"));
const lock = await readJson(resolve(pluginRoot, "package-lock.json"));
const skill = await readFile(resolve(pluginRoot, "skills", "token-shrinker", "SKILL.md"), "utf8");

assert.equal(marketplace.name, "token-shrinker-plugins");
assert.equal(marketplace.plugins.length, 1);
assert.equal(marketplace.plugins[0].name, "token-shrinker");
assert.equal(marketplace.plugins[0].source, "./plugins/claude/token-shrinker");
assert.equal(marketplace.plugins[0].version, workspace.version);

assert.equal(manifest.name, "token-shrinker");
assert.equal(manifest.version, workspace.version);
assert.equal(manifest.skills, "./skills/");
assert.equal(manifest.mcpServers, "./.mcp.json");

assert.equal(pluginPackage.version, workspace.version);
assert.equal(pluginPackage.dependencies["@token-shrinker/cli"], workspace.version);
assert.equal(lock.packages[""].dependencies["@token-shrinker/cli"], workspace.version);

const server = mcp.mcpServers["token-shrinker"];
assert.equal(server.command, "node");
assert.deepEqual(server.args, [
  "${CLAUDE_PLUGIN_ROOT}/node_modules/@token-shrinker/cli/dist/index.js",
  "start",
  "--stdio",
]);
assert.deepEqual(server.env, {});
assert.match(skill, /token-shrinker-owned:v1/);
assert.doesNotMatch(JSON.stringify(mcp), /OPENAI_BASE_URL|ANTHROPIC_BASE_URL|API_KEY/);

console.log("Claude Code plugin marketplace and MCP package verified");
