import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
  AdapterConfigError, applyAdapterPlan, detectAdapter, planAdapter,
} from "../dist/index.js";

const fixtures = JSON.parse(await readFile(new URL("../../../fixtures/adapters/v1.json",
  import.meta.url), "utf8"));

const providerKeys = [
  "ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY", "OPENAI_BASE_URL", "OPENAI_API_KEY",
  "GOOGLE_GEMINI_BASE_URL", "GEMINI_API_KEY",
];

export async function runAdapterContract(definition) {
  const fixture = fixtures.agents[definition.id];
  assert(fixture, `missing committed fixture for ${definition.id}`);
  const root = await mkdtemp(join(tmpdir(), `token shrinker ${definition.id} `));
  const binary = join(root, "bin with spaces", process.platform === "win32" ?
    "token-shrinker.exe" : "token-shrinker");
  const agent = join(root, "fake-path", process.platform === "win32" ?
    `${definition.executable}.EXE` : definition.executable);
  await mkdir(dirname(binary), { recursive: true });
  await mkdir(dirname(agent), { recursive: true });
  await writeFile(binary, "binary"); await writeFile(agent, "agent");
  if (process.platform !== "win32") { await chmod(binary, 0o755); await chmod(agent, 0o755); }
  const context = { root, binaryPath: binary, pathValue: dirname(agent) };
  const config = definition.config(binary);
  assert.equal(config.relativePath, fixture.config);
  const configPath = join(root, config.relativePath);
  let original = null;
  if (config.format === "json") {
    original = JSON.stringify({ nativeProvider: { baseUrl: "https://native.invalid", token: "keep" },
      unrelated: { spacing: true } }, null, 4) + "\n";
  } else if (config.format === "toml-fragment") {
    original = "model_provider = \"native\"\n[user]\ncolor = true\n";
  }
  if (original !== null) {
    await mkdir(dirname(configPath), { recursive: true }); await writeFile(configPath, original);
  }
  const envBefore = Object.fromEntries(providerKeys.map((key) => [key, process.env[key]]));
  const dryRun = await planAdapter(definition, "install", context);
  assert(dryRun.changes.some((change) => change.before !== change.after));
  await assert.rejects(readFile(dryRun.changes.find((change) => change.kind === "skill").path));
  await applyAdapterPlan(dryRun);
  const detection = await detectAdapter(definition, context);
  assert.equal(detection.configured, true);
  assert.equal(detection.executablePath, agent);
  for (const platform of ["win32", "darwin", "linux"]) {
    const conventionRoot = join(root, `path-${platform}`);
    const conventionAgent = join(conventionRoot, platform === "win32"
      ? `${definition.executable}.EXE` : definition.executable);
    await mkdir(conventionRoot, { recursive: true }); await writeFile(conventionAgent, "agent");
    if (process.platform !== "win32") await chmod(conventionAgent, 0o755);
    const convention = await detectAdapter(definition, {
      ...context, platform, pathValue: conventionRoot, pathExt: ".EXE",
    });
    assert.equal(convention.executablePath, conventionAgent, `${platform} executable convention`);
  }
  const once = await readFile(configPath, "utf8");
  await applyAdapterPlan(await planAdapter(definition, "install", context));
  assert.equal(await readFile(configPath, "utf8"), once, "reinstall must be byte-idempotent");
  assert.deepEqual(Object.fromEntries(providerKeys.map((key) => [key, process.env[key]])), envBefore);
  if (config.format === "json") {
    const installed = JSON.parse(once);
    assert.deepEqual(installed.nativeProvider, { baseUrl: "https://native.invalid", token: "keep" });
    assert.deepEqual(installed.unrelated, { spacing: true });
  }
  await applyAdapterPlan(await planAdapter(definition, "uninstall", context));
  const removed = await detectAdapter(definition, context);
  assert.equal(removed.configured, false);
  if (original !== null) {
    const restored = await readFile(configPath, "utf8");
    if (config.format === "json") assert.deepEqual(JSON.parse(restored), JSON.parse(original));
    else assert.equal(restored, original);
  }

  const malformedRoot = await mkdtemp(join(tmpdir(), `token malformed ${definition.id} `));
  const malformedPath = join(malformedRoot, config.relativePath);
  await mkdir(dirname(malformedPath), { recursive: true });
  await writeFile(malformedPath, fixture.malformed);
  await assert.rejects(planAdapter(definition, "install", { ...context, root: malformedRoot }),
    (error) => error instanceof AdapterConfigError);
  if (fixture.partial) {
    const partialRoot = await mkdtemp(join(tmpdir(), `token partial ${definition.id} `));
    const partialPath = join(partialRoot, config.relativePath);
    await mkdir(dirname(partialPath), { recursive: true });
    await writeFile(partialPath, fixture.partial);
    await assert.rejects(planAdapter(definition, "install", { ...context, root: partialRoot }),
      (error) => error instanceof AdapterConfigError);
  }
}
