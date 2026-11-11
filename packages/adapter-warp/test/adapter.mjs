import test from "node:test";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { warpAdapter } from "../dist/index.js";
test("Warp adapter lifecycle preserves native provider transport", async () =>
  runAdapterContract(warpAdapter));
