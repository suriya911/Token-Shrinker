import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { openCodeAdapter } from "../dist/index.js";
test("OpenCode adapter lifecycle uses current mcp server shape", async () =>
  runAdapterContract(openCodeAdapter));
