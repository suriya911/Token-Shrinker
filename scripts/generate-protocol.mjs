import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import process from "node:process";

const binary = process.env.TOKEN_SHRINKER_BINARY ??
  resolve("target", "debug", process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");
const versionRun = spawnSync(binary, ["version", "--json"], { encoding: "utf8" });
const referenceRun = spawnSync(binary, ["reference", "--json"], { encoding: "utf8" });
assert.equal(versionRun.status, 0, versionRun.stderr);
assert.equal(referenceRun.status, 0, referenceRun.stderr);
const version = JSON.parse(versionRun.stdout);
const reference = JSON.parse(referenceRun.stdout);
const toolNames = reference.tools.map((tool) => "  | " + JSON.stringify(tool.name)).join("\n");
const content = [
  "/** Generated from Token-Shrinker's Rust public protocol metadata. Do not hand edit. */",
  "export const DOMAIN_PROTOCOL_VERSION = " + JSON.stringify(version.protocolVersion) + " as const;",
  "export const PUBLIC_SCHEMA_VERSION = " + version.schemaVersion + " as const;",
  "export const MCP_PROTOCOL_VERSION = " + JSON.stringify(reference.tools.length ? "2025-11-25" : "") + " as const;",
  "export type ToolName =",
  toolNames + ";",
  "export type RouteMode = \"FAST\" | \"BUILD\" | \"DEEP\";",
  "export type OutputMode = \"lite\" | \"full\" | \"ultra\" | \"wenyan-lite\" | \"wenyan-full\" | \"wenyan-ultra\" | \"off\";",
  "export interface PublicEnvelope<T extends object> {",
  "  protocolVersion: string; requestId: string; warnings: string[]; data: T;",
  "}",
  "export interface CapabilityReport {",
  "  binaryVersion: string; packageVersion: string; protocolVersion: string;",
  "  mcpProtocolVersion: string; schemaVersion: number;",
  "  health: \"healthy\" | \"degraded\" | \"failed\";",
  "  capabilities: Array<{ id: string; provider: string; fallback?: string | null;",
  "    health: \"healthy\" | \"degraded\" | \"failed\"; warningCode?: string | null }>;",
  "  tools: ToolName[];",
  "}",
  "export interface BuildContextRequest { root: string; goal: string; budget: number }",
  "export interface FormatFinalRequest { text: string; mode?: OutputMode; agent?: string; tool?: string }",
  "export interface CallOptions { timeoutMs?: number; signal?: AbortSignal }",
  "",
].join("\n");
const path = "packages/sdk/src/protocol.ts";
if (process.argv.includes("--check")) {
  assert.equal(await readFile(path, "utf8"), content, path + " is stale");
} else {
  await writeFile(path, content);
}
