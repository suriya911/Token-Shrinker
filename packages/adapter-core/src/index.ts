/** Transactional, native-transport-safe contracts for agent configuration adapters. */
import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { access, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { delimiter, dirname, extname, join, resolve } from "node:path";
import process from "node:process";
import { isDeepStrictEqual } from "node:util";
import { TokenShrinkerClient } from "@token-shrinker/sdk";

export type AdapterId = "claude-code" | "codex" | "gemini" | "opencode" | "aider";
export type AdapterAction = "install" | "uninstall";
export type ConfigFormat = "json" | "owned-text" | "toml-fragment";

export interface ManagedConfig {
  relativePath: string;
  format: ConfigFormat;
  content?: string;
  jsonPath?: readonly string[];
  jsonValue?: Readonly<Record<string, unknown>>;
}

export interface AdapterDefinition {
  id: AdapterId;
  displayName: string;
  executable: string;
  docsUrl: string;
  testedVersions: string;
  integration: "mcp-stdio" | "aider-context";
  config(binaryPath: string): ManagedConfig;
  skillRelativePath: string;
  additionalFiles?(binaryPath: string): readonly ManagedConfig[];
}

export interface AdapterContext {
  root: string;
  binaryPath?: string;
  pathValue?: string;
  platform?: NodeJS.Platform;
  pathExt?: string;
}

export interface AdapterDetection {
  adapter: AdapterId;
  executablePath: string | null;
  configPath: string;
  skillPath: string;
  configExists: boolean;
  configured: boolean;
}

export type ClientApprovalState = "approved" | "required" | "not-applicable" | "unknown";

export interface AdapterClientState {
  approval: ClientApprovalState;
  connection: "not-checked";
  detail: string;
}

export interface PlannedFileChange {
  path: string;
  kind: "config" | "skill" | "support";
  before: string | null;
  after: string | null;
}

export interface AdapterPlan {
  adapter: AdapterId;
  action: AdapterAction;
  changes: readonly PlannedFileChange[];
  nativeTransportUnchanged: true;
  validation: {
    command: string;
    args: readonly string[];
    tools: readonly ["token_shrinker_capabilities", "token_shrinker_build_context"];
  };
}

export class AdapterConfigError extends Error {
  public constructor(message: string, public readonly code:
    "malformed-config" | "ownership-conflict" | "native-transport-mutation") {
    super(message); this.name = new.target.name;
  }
}

const SKILL = `---
name: token-shrinker
description: Use Token-Shrinker to route work, build bounded evidence context, fetch omitted sources, inspect warnings, and request approved execution.
license: Apache-2.0
---
<!-- token-shrinker-owned:v1 -->

# Token-Shrinker workflow

1. Call \`token_shrinker_capabilities\` before relying on optional providers or execution.
2. Call \`token_shrinker_task_status\` with the workspace root at the start of every task/session. If no ledger exists, call \`token_shrinker_task_update\` with \`action: "ensure"\`, then add the task.
3. Start the active task with \`token_shrinker_task_update\` (\`action: "start"\`) before implementation; mark it \`complete\` or \`block\` before the final response.
4. Call \`token_shrinker_route\` when the appropriate FAST, BUILD, or DEEP route is unclear.
5. Call \`token_shrinker_build_context\` with the repository root, concrete goal, and explicit token budget.
6. Treat only returned bundle item content as evidence. A source ID or omission is not evidence by itself.
7. If an omitted source may be required for correctness, call \`token_shrinker_fetch_source\` with its exact source ID before answering. If retrieval fails or the content still does not establish the claim, say the claim is unverified.
8. Cite only content actually returned by \`token_shrinker_build_context\` or \`token_shrinker_fetch_source\`. Never infer implementation details from filenames, source IDs, package metadata, or prior knowledge.
9. Inspect every warning and preserve relevant warnings, citations, commands, exact errors, and uncertainty.
10. When the model/provider reports usage from a compatible tokenizer, call \`token_shrinker_record_tokens\` with raw and optimized counts, tokenizer ID, and \`precision: "exact"\`; otherwise preserve \`estimated\` measurements.
11. Call \`token_shrinker_execute\` only after the user approves the exact command and policy permits it.
12. Use \`token_shrinker_format_final\` only for the final human response. Never apply concise formatting to tool calls, JSON, code, commands, logs, citations, or intermediate evidence.

If Token-Shrinker is unavailable, say that the \`token-shrinker\` executable is missing and ask the user to install it or run \`token-shrinker doctor\`. Never redirect provider endpoints, credentials, or native model transport.
`;

const AIDER_CONTEXT = `<!-- token-shrinker-owned:v1 -->
# Token-Shrinker with Aider

Build context before launch with \`token-shrinker context build --root . --goal "<goal>" --budget 16000 --json\` and provide the selected evidence as read-only context. Preserve warnings, citations, commands, exact errors, and uncertainty. Concise formatting applies only to the final human response. Never change provider endpoints or credentials.
`;

export function portableSkill(): string { return SKILL; }
export function aiderContext(): string { return AIDER_CONTEXT; }

export async function detectAdapter(definition: AdapterDefinition,
  context: AdapterContext): Promise<AdapterDetection> {
  const binary = context.binaryPath ?? "token-shrinker";
  const config = definition.config(binary);
  const configPath = resolve(context.root, config.relativePath);
  const skillPath = resolve(context.root, definition.skillRelativePath);
  const existing = await readOptional(configPath);
  return {
    adapter: definition.id,
    executablePath: await findExecutable(definition.executable, context),
    configPath,
    skillPath,
    configExists: existing !== null,
    configured: existing !== null && ownsConfig(config, existing),
  };
}

export async function planAdapter(definition: AdapterDefinition, action: AdapterAction,
  context: AdapterContext): Promise<AdapterPlan> {
  const binary = context.binaryPath ?? "token-shrinker";
  const managed = [definition.config(binary), ...(definition.additionalFiles?.(binary) ?? [])];
  const changes: PlannedFileChange[] = [];
  for (const [index, config] of managed.entries()) {
    const path = resolve(context.root, config.relativePath);
    const before = await readOptional(path);
    const after = mutateConfig(config, before, action);
    changes.push({ path, kind: index === 0 ? "config" : "support", before, after });
  }
  const skillPath = resolve(context.root, definition.skillRelativePath);
  const skillBefore = await readOptional(skillPath);
  const skillAfter = mutateOwnedText(skillBefore, SKILL, action, skillPath);
  changes.push({ path: skillPath, kind: "skill", before: skillBefore, after: skillAfter });
  return {
    adapter: definition.id,
    action,
    changes,
    nativeTransportUnchanged: true,
    validation: { command: binary, args: ["start", "--stdio"],
      tools: ["token_shrinker_capabilities", "token_shrinker_build_context"] },
  };
}

export async function applyAdapterPlan(plan: AdapterPlan): Promise<void> {
  const changed = plan.changes.filter((change) => change.before !== change.after);
  const applied: PlannedFileChange[] = [];
  try {
    for (const change of changed) {
      await writeState(change.path, change.after);
      applied.push(change);
    }
  } catch (error) {
    for (const change of applied.reverse()) await writeState(change.path, change.before);
    throw error;
  }
}

export async function validateAdapter(definition: AdapterDefinition,
  context: AdapterContext): Promise<{ capabilities: true; buildContext: true }> {
  if (definition.integration !== "mcp-stdio") {
    const config = definition.config(context.binaryPath ?? "token-shrinker");
    const installed = await readOptional(resolve(context.root, config.relativePath));
    if (installed === null || !ownsConfig(config, installed)) {
      throw new AdapterConfigError("Aider context configuration is not installed", "ownership-conflict");
    }
  }
  const client = await TokenShrinkerClient.connect(context.binaryPath
    ? { transport: "stdio", binaryPath: context.binaryPath }
    : { transport: "stdio" });
  try {
    const capabilities = await client.capabilities({ timeoutMs: 10_000 });
    const bundle = await client.buildContext({ root: context.root,
      goal: "validate the Token-Shrinker agent adapter", budget: 2_000 }, { timeoutMs: 10_000 });
    if (!capabilities.data || !bundle.data) throw new Error("adapter validation returned no data");
    return { capabilities: true, buildContext: true };
  } finally { await client.close(); }
}

/** Reports visible client approval state without changing or bypassing client consent. */
export async function inspectAdapterClientState(definition: AdapterDefinition,
  context: AdapterContext): Promise<AdapterClientState> {
  if (definition.integration === "aider-context") return {
    approval: "not-applicable", connection: "not-checked",
    detail: "Aider uses generated context rather than a persistent MCP server.",
  };
  if (definition.id !== "claude-code") return {
    approval: "unknown", connection: "not-checked",
    detail: "Server protocol was validated directly; client approval and connection were not checked.",
  };
  const settings = await readOptional(resolve(context.root, ".claude/settings.local.json"));
  if (settings === null) return {
    approval: "unknown", connection: "not-checked",
    detail: "Claude project approval has not been recorded in .claude/settings.local.json.",
  };
  const parsed = parseJsonObject(settings, ".claude/settings.local.json");
  const disabled = stringArray(parsed.disabledMcpjsonServers);
  const enabled = stringArray(parsed.enabledMcpjsonServers);
  if (disabled.includes("token-shrinker")) return {
    approval: "required", connection: "not-checked",
    detail: "Claude rejected the project MCP server. Run `claude mcp reset-project-choices`, restart Claude, and approve token-shrinker.",
  };
  if (enabled.includes("token-shrinker")) return {
    approval: "approved", connection: "not-checked",
    detail: "Claude project settings record token-shrinker as approved; live client connection was not checked.",
  };
  return {
    approval: "unknown", connection: "not-checked",
    detail: "Claude approval is not explicit in project-local settings; confirm with `claude mcp get token-shrinker`.",
  };
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function mutateConfig(config: ManagedConfig, before: string | null,
  action: AdapterAction): string | null {
  if (config.format === "owned-text") {
    return mutateOwnedText(before, required(config.content, "owned text content"), action,
      config.relativePath);
  }
  if (config.format === "toml-fragment") {
    return mutateToml(before, required(config.content, "TOML fragment"), action);
  }
  const original = parseJsonObject(before, config.relativePath);
  const next = structuredClone(original);
  const path = config.jsonPath ?? [];
  if (path.length === 0) throw new AdapterConfigError("JSON adapter path is empty", "malformed-config");
  if (action === "install") setJsonPath(next, path, config.jsonValue ?? {});
  else deleteJsonPath(next, path, config.jsonValue ?? {});
  assertOnlyOwnedJsonChanged(original, next, path);
  return JSON.stringify(next, null, 2) + "\n";
}

function parseJsonObject(content: string | null, path: string): Record<string, unknown> {
  if (content === null || content.trim() === "") return {};
  try {
    const value: unknown = JSON.parse(content);
    if (!isRecord(value)) throw new Error("root must be an object");
    return value;
  } catch (error) {
    throw new AdapterConfigError(`Malformed JSON configuration at ${path}: ${String(error)}`,
      "malformed-config");
  }
}

function setJsonPath(root: Record<string, unknown>, path: readonly string[], value: object): void {
  let cursor = root;
  for (const segment of path.slice(0, -1)) {
    const existing = cursor[segment];
    if (existing === undefined) cursor[segment] = {};
    else if (!isRecord(existing)) throw new AdapterConfigError(
      `Configuration key ${segment} is not an object`, "ownership-conflict");
    cursor = cursor[segment] as Record<string, unknown>;
  }
  const key = path.at(-1) as string;
  const existing = cursor[key];
  if (existing !== undefined && !deepEqual(existing, value)) throw new AdapterConfigError(
    `Configuration key ${path.join(".")} is owned by another value`, "ownership-conflict");
  cursor[key] = structuredClone(value);
}

function deleteJsonPath(root: Record<string, unknown>, path: readonly string[], value: object): void {
  const parents: Array<[Record<string, unknown>, string]> = [];
  let cursor = root;
  for (const segment of path.slice(0, -1)) {
    const next = cursor[segment];
    if (next === undefined) return;
    if (!isRecord(next)) throw new AdapterConfigError(
      `Configuration key ${segment} is not an object`, "ownership-conflict");
    parents.push([cursor, segment]); cursor = next;
  }
  const key = path.at(-1) as string;
  if (cursor[key] === undefined) return;
  if (!deepEqual(cursor[key], value)) throw new AdapterConfigError(
    `Refusing to remove modified configuration key ${path.join(".")}`, "ownership-conflict");
  delete cursor[key];
  for (const [parent, segment] of parents.reverse()) {
    const child = parent[segment];
    if (isRecord(child) && Object.keys(child).length === 0) delete parent[segment];
    else break;
  }
}

function assertOnlyOwnedJsonChanged(before: Record<string, unknown>, after: Record<string, unknown>,
  path: readonly string[]): void {
  const cleanBefore = structuredClone(before); const cleanAfter = structuredClone(after);
  deleteJsonPathUnchecked(cleanBefore, path); deleteJsonPathUnchecked(cleanAfter, path);
  if (!deepEqual(cleanBefore, cleanAfter)) throw new AdapterConfigError(
    "Adapter attempted to alter native transport or unrelated configuration",
    "native-transport-mutation");
}

function deleteJsonPathUnchecked(root: Record<string, unknown>, path: readonly string[]): void {
  const parents: Array<[Record<string, unknown>, string]> = [];
  let cursor: Record<string, unknown> = root;
  for (const segment of path.slice(0, -1)) {
    if (!isRecord(cursor[segment])) return;
    parents.push([cursor, segment]);
    cursor = cursor[segment] as Record<string, unknown>;
  }
  delete cursor[path.at(-1) as string];
  for (const [parent, segment] of parents.reverse()) {
    const child = parent[segment];
    if (isRecord(child) && Object.keys(child).length === 0) delete parent[segment];
    else break;
  }
}

const START = "# >>> token-shrinker: owned adapter fragment >>>";
const END = "# <<< token-shrinker: owned adapter fragment <<<";
function mutateToml(before: string | null, fragment: string, action: AdapterAction): string | null {
  const source = before ?? "";
  if (!isPlausibleToml(source) || source.includes(START) !== source.includes(END)) {
    throw new AdapterConfigError("Malformed or partial Token-Shrinker TOML fragment",
      "malformed-config");
  }
  const block = `${START}\n${fragment.trim()}\n${END}`;
  const start = source.indexOf(START); const end = source.indexOf(END);
  if (start >= 0 && end < start) throw new AdapterConfigError(
    "Malformed Token-Shrinker TOML marker order", "malformed-config");
  if (action === "install") {
    if (start >= 0) {
      const existing = source.slice(start, end + END.length);
      if (existing !== block) throw new AdapterConfigError(
        "Existing Token-Shrinker TOML fragment was modified", "ownership-conflict");
      return source;
    }
    return source.trimEnd() + (source.trim() ? "\n\n" : "") + block + "\n";
  }
  if (start < 0) return before;
  const prefix = source.slice(0, start).trimEnd();
  const suffix = source.slice(end + END.length).trimStart();
  return [prefix, suffix].filter(Boolean).join("\n\n") + (prefix || suffix ? "\n" : "");
}

function isPlausibleToml(source: string): boolean {
  if (source.includes("\0")) return false;
  let squareDepth = 0; let quote: "basic" | "literal" | null = null;
  let triple = false; let escaped = false; let comment = false;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (comment) { if (character === "\n") comment = false; continue; }
    if (quote === null && character === "#") { comment = true; continue; }
    const marker = quote === "basic" ? '"' : "'";
    if (quote !== null) {
      if (quote === "basic" && !triple && character === "\\" && !escaped) {
        escaped = true; continue;
      }
      if (character === marker && !escaped) {
        if (triple && source.slice(index, index + 3) === marker.repeat(3)) {
          quote = null; triple = false; index += 2;
        } else if (!triple) quote = null;
      } else if (!triple && character === "\n") return false;
      escaped = false; continue;
    }
    if (character === '"' || character === "'") {
      quote = character === '"' ? "basic" : "literal";
      triple = source.slice(index, index + 3) === character.repeat(3);
      if (triple) index += 2;
    } else if (character === "[") squareDepth += 1;
    else if (character === "]") { squareDepth -= 1; if (squareDepth < 0) return false; }
  }
  return quote === null && squareDepth === 0;
}

function mutateOwnedText(before: string | null, owned: string, action: AdapterAction,
  path: string): string | null {
  if (action === "install") {
    if (before !== null && before !== owned &&
      !before.includes("<!-- token-shrinker-owned:")) throw new AdapterConfigError(
      `Refusing to overwrite non-owned file ${path}`, "ownership-conflict");
    return owned;
  }
  if (before === null) return null;
  if (before !== owned) throw new AdapterConfigError(
    `Refusing to remove modified file ${path}`, "ownership-conflict");
  return null;
}

function ownsConfig(config: ManagedConfig, content: string): boolean {
  if (config.format === "owned-text") return content === config.content;
  if (config.format === "toml-fragment") return content.includes(START) && content.includes(END);
  try {
    let value: unknown = JSON.parse(content);
    for (const segment of config.jsonPath ?? []) {
      if (!isRecord(value)) return false;
      value = value[segment];
    }
    return deepEqual(value, config.jsonValue);
  } catch { return false; }
}

async function findExecutable(name: string, context: AdapterContext): Promise<string | null> {
  if (name.includes("/") || name.includes("\\")) return await canExecute(name) ? resolve(name) : null;
  const platform = context.platform ?? process.platform;
  const extensions = platform === "win32"
    ? (context.pathExt ?? process.env.PATHEXT ?? ".EXE;.CMD;.BAT;.COM").split(";") : [""];
  for (const directory of (context.pathValue ?? process.env.PATH ?? "").split(delimiter)) {
    if (!directory) continue;
    for (const extension of extensions) {
      const candidate = join(directory, platform === "win32" && !extname(name) ? name + extension : name);
      if (await canExecute(candidate)) return candidate;
    }
  }
  return null;
}

async function canExecute(path: string): Promise<boolean> {
  try { await access(path, process.platform === "win32" ? constants.F_OK : constants.X_OK); return true; }
  catch { return false; }
}
async function readOptional(path: string): Promise<string | null> {
  try { return await readFile(path, "utf8"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return null; throw error; }
}
async function writeState(path: string, content: string | null): Promise<void> {
  if (content === null) { await rm(path, { force: true }); return; }
  await mkdir(dirname(path), { recursive: true }); await writeFile(path, content, "utf8");
}
function required(value: string | undefined, name: string): string {
  if (value === undefined) throw new AdapterConfigError(`Missing ${name}`, "malformed-config");
  return value;
}
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function deepEqual(left: unknown, right: unknown): boolean {
  return isDeepStrictEqual(left, right);
}
export function sha256(content: string): string {
  return createHash("sha256").update(content).digest("hex");
}
export function tomlString(value: string): string {
  return JSON.stringify(value).replaceAll("\\/", "/");
}
