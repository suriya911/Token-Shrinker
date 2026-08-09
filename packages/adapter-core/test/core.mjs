import test from "node:test";
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aiderContext, applyAdapterPlan, inspectAdapterClientState, portableSkill, sha256,
  tomlString } from
  "../dist/index.js";

test("portable skill preserves evidence and scopes concise formatting", () => {
  const skill = portableSkill();
  assert(skill.startsWith("---\nname: token-shrinker\n"));
  for (const required of ["warnings", "citations", "commands", "uncertainty",
    "only for the final human response", "user approves", "native model transport",
    "A source ID or omission is not evidence", "Never infer implementation details"]) {
    assert(skill.includes(required), required);
  }
  assert(aiderContext().includes("read-only context"));
});

test("Claude rejection is reported without changing approval", async () => {
  const root = await mkdtemp(join(tmpdir(), "token-shrinker-claude-state-"));
  try {
    await mkdir(join(root, ".claude"));
    const settings = join(root, ".claude", "settings.local.json");
    const original = JSON.stringify({ disabledMcpjsonServers: ["token-shrinker"] }, null, 2);
    await writeFile(settings, original);
    const state = await inspectAdapterClientState({
      id: "claude-code", integration: "mcp-stdio",
    }, { root });
    assert.equal(state.approval, "required");
    assert.match(state.detail, /reset-project-choices/);
    assert.equal(await readFile(settings, "utf8"), original);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("stable helpers escape paths and hash owned content", () => {
  assert.equal(tomlString("C:\\Program Files\\Token Shrinker\\token-shrinker.exe"),
    '"C:\\\\Program Files\\\\Token Shrinker\\\\token-shrinker.exe"');
  assert.equal(sha256("owned"), sha256("owned"));
  assert.notEqual(sha256("owned"), sha256("changed"));
});

test("failed multi-file application rolls back earlier writes", async () => {
  const root = await mkdtemp(join(tmpdir(), "token-shrinker-rollback-"));
  const first = join(root, "first.txt"); const blockedParent = join(root, "blocked");
  await writeFile(first, "before"); await writeFile(blockedParent, "not-a-directory");
  await assert.rejects(applyAdapterPlan({
    adapter: "codex", action: "install", nativeTransportUnchanged: true,
    validation: { command: "token-shrinker", args: ["start", "--stdio"],
      tools: ["token_shrinker_capabilities", "token_shrinker_build_context"] },
    changes: [
      { path: first, kind: "config", before: "before", after: "after" },
      { path: join(blockedParent, "second.txt"), kind: "skill", before: null, after: "owned" },
    ],
  }));
  assert.equal(await readFile(first, "utf8"), "before");
});
