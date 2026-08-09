#!/usr/bin/env node
/** Thin, downloader-free launcher for platform-specific native packages. */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { realpathSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import {
  applyAdapterPlan,
  inspectAdapterClientState,
  planAdapter,
  validateAdapter,
  type AdapterDefinition,
  type AdapterPlan,
  type ClientApprovalState,
} from "@token-shrinker/adapter-core";
import { aiderAdapter } from "@token-shrinker/adapter-aider";
import { claudeCodeAdapter } from "@token-shrinker/adapter-claude-code";
import { codexAdapter } from "@token-shrinker/adapter-codex";
import { geminiAdapter } from "@token-shrinker/adapter-gemini";
import { openCodeAdapter } from "@token-shrinker/adapter-opencode";

const require = createRequire(import.meta.url);
const PLATFORM_PACKAGES: Readonly<Record<string, string>> = {
  "darwin-arm64": "@token-shrinker/cli-darwin-arm64",
  "darwin-x64": "@token-shrinker/cli-darwin-x64",
  "linux-x64": "@token-shrinker/cli-linux-x64-gnu",
  "win32-x64": "@token-shrinker/cli-win32-x64",
};

const ADAPTERS: Readonly<Record<string, AdapterDefinition>> = {
  aider: aiderAdapter,
  claude: claudeCodeAdapter,
  "claude-code": claudeCodeAdapter,
  codex: codexAdapter,
  gemini: geminiAdapter,
  opencode: openCodeAdapter,
};

export interface AdapterCommandOptions {
  binaryPath?: string;
  cwd?: string;
  validate?: boolean;
}

export interface AdapterCommandResult {
  adapter: string;
  action: "install" | "uninstall";
  applied: boolean;
  nativeTransportUnchanged: true;
  /** Direct Token-Shrinker stdio protocol validation, not an agent-client connection check. */
  serverProtocolValidated: boolean;
  clientApproval: ClientApprovalState;
  clientConnection: "not-checked";
  clientStateDetail: string;
  changes: ReadonlyArray<{ path: string; kind: string; operation: string }>;
}

export function platformPackage(platform = process.platform, arch = process.arch): string {
  const key = platform + "-" + arch;
  const packageName = PLATFORM_PACKAGES[key];
  if (!packageName) {
    throw new Error("Token-Shrinker has no native package for " + key +
      ". Supported targets: " + Object.keys(PLATFORM_PACKAGES).join(", "));
  }
  return packageName;
}

export function resolveBinary(): string {
  if (process.env.TOKEN_SHRINKER_BINARY) return process.env.TOKEN_SHRINKER_BINARY;
  const packageName = platformPackage();
  try {
    const packageJson = require.resolve(packageName + "/package.json");
    const executable = process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker";
    return join(dirname(packageJson), "bin", executable);
  } catch (error) {
    throw new Error("The optional native package " + packageName +
      " is missing. Reinstall @token-shrinker/cli with optional dependencies enabled.",
      { cause: error });
  }
}

export async function verifyBinaryChecksum(binaryPath: string, expectedSha256: string):
  Promise<boolean> {
  const observed = createHash("sha256").update(await readFile(binaryPath)).digest("hex");
  if (!/^[0-9a-f]{64}$/.test(expectedSha256)) return false;
  let difference = observed.length ^ expectedSha256.length;
  for (let index = 0; index < observed.length; index += 1) {
    difference |= observed.charCodeAt(index) ^ expectedSha256.charCodeAt(index);
  }
  return difference === 0;
}

/** Applies a tested project-scoped agent adapter without changing model transport. */
export async function adapterCommand(args: readonly string[], options: AdapterCommandOptions = {}):
  Promise<AdapterCommandResult | null> {
  const verb = args[0];
  if (verb !== "add" && verb !== "remove") return null;
  const requested = args[1];
  const definition = requested ? ADAPTERS[requested] : undefined;
  if (!definition) {
    throw new Error("Unknown agent adapter. Supported adapters: " +
      ["aider", "claude-code", "codex", "gemini", "opencode"].join(", "));
  }
  const action = verb === "add" ? "install" : "uninstall";
  const root = option(args, "--root") ?? options.cwd ?? process.cwd();
  const binaryPath = resolve(option(args, "--binary") ?? options.binaryPath ?? resolveBinary());
  const context = { root, binaryPath };
  const plan = await planAdapter(definition, action, context);
  const changes = summarizeChanges(plan);
  const dryRun = args.includes("--dry-run");
  if (dryRun) return {
    adapter: definition.id, action, applied: false, nativeTransportUnchanged: true,
    serverProtocolValidated: false, clientApproval: "unknown", clientConnection: "not-checked",
    clientStateDetail: "Dry run only; server protocol and agent-client state were not checked.", changes,
  };

  await applyAdapterPlan(plan);
  let serverProtocolValidated = false;
  if (action === "install" && options.validate !== false) {
    try {
      await validateAdapter(definition, context);
      serverProtocolValidated = true;
    } catch (error) {
      await applyAdapterPlan(reversePlan(plan));
      throw new Error(`Adapter validation failed; all changes were rolled back: ${String(error)}`);
    }
  }
  const clientState = await inspectAdapterClientState(definition, context);
  return {
    adapter: definition.id, action, applied: true, nativeTransportUnchanged: true,
    serverProtocolValidated, clientApproval: clientState.approval,
    clientConnection: clientState.connection, clientStateDetail: clientState.detail, changes,
  };
}

export async function launch(args = process.argv.slice(2)): Promise<number> {
  const child = spawn(resolveBinary(), args, { stdio: "inherit", windowsHide: false });
  const forward = (signal: NodeJS.Signals): void => {
    if (!child.killed) child.kill(signal);
  };
  process.once("SIGINT", forward);
  process.once("SIGTERM", forward);
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      process.removeListener("SIGINT", forward);
      process.removeListener("SIGTERM", forward);
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

const invokedPath = process.argv[1];
if (invokedPath && realpathSync(invokedPath) === realpathSync(fileURLToPath(import.meta.url))) {
  const args = process.argv.slice(2);
  adapterCommand(args).then(async (result) => {
    if (result) {
      console.log(args.includes("--json") ? JSON.stringify(result) : renderAdapterResult(result));
      return 0;
    }
    return launch(args);
  }).then((code) => { process.exitCode = code; }).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}

function option(args: readonly string[], name: string): string | undefined {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
}

function summarizeChanges(plan: AdapterPlan): AdapterCommandResult["changes"] {
  return plan.changes.filter((change) => change.before !== change.after).map((change) => ({
    path: change.path,
    kind: change.kind,
    operation: change.after === null ? "remove" : change.before === null ? "create" : "update",
  }));
}

function reversePlan(plan: AdapterPlan): AdapterPlan {
  return {
    ...plan,
    changes: plan.changes.map((change) => ({
      ...change,
      before: change.after,
      after: change.before,
    })),
  };
}

function renderAdapterResult(result: AdapterCommandResult): string {
  const state = result.applied ? "applied" : "preview";
  const lines = result.changes.map((change) =>
    `- ${change.operation} ${change.kind}: ${change.path}`);
  return [`Token-Shrinker ${result.adapter} adapter ${state}.`,
    `Native model transport unchanged: ${result.nativeTransportUnchanged ? "yes" : "no"}`,
    `Server protocol validated directly: ${result.serverProtocolValidated ? "yes" : "no"}`,
    `Agent approval: ${result.clientApproval}; live connection: ${result.clientConnection}`,
    result.clientStateDetail,
    ...lines].join("\n");
}
