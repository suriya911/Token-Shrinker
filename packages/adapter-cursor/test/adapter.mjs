import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { cursorAdapter } from "../dist/index.js";
test("Cursor adapter lifecycle preserves native provider transport", async () =>
  runAdapterContract(cursorAdapter));
