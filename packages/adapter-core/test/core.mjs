import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aiderContext, applyAdapterPlan, portableSkill, sha256, tomlString } from
  "../dist/index.js";

test("portable skill preserves evidence and scopes concise formatting", () => {
  const skill = portableSkill();
  for (const required of ["warnings", "citations", "commands", "uncertainty",
    "only for the final human response", "user approves", "native model transport"]) {
    assert(skill.includes(required), required);
  }
  assert(aiderContext().includes("read-only context"));
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
