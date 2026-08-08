import assert from "node:assert/strict";
import { access, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import process from "node:process";
import { applyAdapterPlan, planAdapter, validateAdapter } from
  "../packages/adapter-core/dist/index.js";
import { claudeCodeAdapter } from "../packages/adapter-claude-code/dist/index.js";
import { codexAdapter } from "../packages/adapter-codex/dist/index.js";
import { geminiAdapter } from "../packages/adapter-gemini/dist/index.js";
import { openCodeAdapter } from "../packages/adapter-opencode/dist/index.js";
import { aiderAdapter } from "../packages/adapter-aider/dist/index.js";

const binary = process.env.TOKEN_SHRINKER_BINARY ?? resolve("target", "debug",
  process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");
await access(binary);
for (const adapter of [claudeCodeAdapter, codexAdapter, geminiAdapter, openCodeAdapter,
  aiderAdapter]) {
  const root = await mkdtemp(join(tmpdir(), `token-shrinker-live-${adapter.id}-`));
  const context = { root, binaryPath: binary };
  await applyAdapterPlan(await planAdapter(adapter, "install", context));
  assert.deepEqual(await validateAdapter(adapter, context),
    { capabilities: true, buildContext: true });
  await applyAdapterPlan(await planAdapter(adapter, "uninstall", context));
}
console.log("all five adapter client paths invoked capabilities and build_context");
