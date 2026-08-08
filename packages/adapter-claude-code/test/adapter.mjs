import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { claudeCodeAdapter } from "../dist/index.js";
test("Claude Code adapter lifecycle preserves native Anthropic transport", async () =>
  runAdapterContract(claudeCodeAdapter));
