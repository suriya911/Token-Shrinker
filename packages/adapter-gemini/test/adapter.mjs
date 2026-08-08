import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { geminiAdapter } from "../dist/index.js";
test("Gemini adapter lifecycle preserves user configuration", async () =>
  runAdapterContract(geminiAdapter));
