import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { openCodeAdapter } from "../dist/index.js";
test("OpenCode V2 adapter lifecycle uses mcp.servers", async () =>
  runAdapterContract(openCodeAdapter));
