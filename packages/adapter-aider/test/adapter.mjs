import test from "node:test";
import assert from "node:assert/strict";
import { runAdapterContract } from "../../adapter-core/test/harness.mjs";
import { aiderAdapter, aiderLaunchArgs } from "../dist/index.js";
test("Aider adapter lifecycle owns its config and context", async () =>
  runAdapterContract(aiderAdapter));
test("Aider launch is explicit and does not wrap model transport", () =>
  assert.deepEqual(aiderLaunchArgs("project with spaces"),
    ["--config", "project with spaces/.token-shrinker/aider.conf.yml"]));
