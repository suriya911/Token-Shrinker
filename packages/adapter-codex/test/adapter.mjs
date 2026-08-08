import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { codexAdapter } from "../dist/index.js";
test("Codex adapter lifecycle owns only its MCP fragment", async () =>
  runAdapterContract(codexAdapter));
