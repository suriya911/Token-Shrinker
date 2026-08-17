/** Typed Node.js client for Token-Shrinker daemon and MCP stdio transports. */
import { randomUUID } from "node:crypto";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { readFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { createInterface, type Interface } from "node:readline";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { DOMAIN_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION, type BuildContextRequest,
  type CallOptions, type CapabilityReport, type FormatFinalRequest,
  type PublicEnvelope } from "./protocol.js";
export * from "./protocol.js";

interface DiscoveryState {
  endpoint: string; pid: number; startedAtMs: number;
  protocolVersion: { major: number; minor: number }; authToken: string;
}
interface RpcResponse {
  jsonrpc: "2.0"; protocolVersion: { major: number; minor: number };
  result?: unknown; error?: { code: number; message: string; dataCode?: string | null };
}
export interface TokenShrinkerTransport {
  call<T extends object>(method: string, params: object, options?: CallOptions):
    Promise<PublicEnvelope<T>>;
  close(): Promise<void>;
}
export class TokenShrinkerError extends Error {
  public constructor(message: string, public readonly code: string,
    public readonly cause?: unknown) {
    super(message); this.name = new.target.name;
  }
}
export class TokenShrinkerTimeoutError extends TokenShrinkerError {
  public constructor() { super("Token-Shrinker request exceeded its deadline", "deadline-exceeded"); }
}
export class TokenShrinkerCancelledError extends TokenShrinkerError {
  public constructor() { super("Token-Shrinker request was cancelled", "cancelled"); }
}
export class TokenShrinkerRemoteError extends TokenShrinkerError {
  public constructor(public readonly rpcCode: number, code: string, message: string) {
    super(message, code);
  }
}
export interface DaemonTransportOptions { discoveryPath?: string; maxFrameBytes?: number }
export class DaemonTransport implements TokenShrinkerTransport {
  readonly #discoveryPath: string;
  readonly #maxFrameBytes: number;
  #discovery: DiscoveryState | undefined;
  public constructor(options: DaemonTransportOptions = {}) {
    this.#discoveryPath = options.discoveryPath ?? defaultDiscoveryPath();
    this.#maxFrameBytes = options.maxFrameBytes ?? 8 * 1024 * 1024;
  }
  public async call<T extends object>(method: string, params: object,
    options: CallOptions = {}): Promise<PublicEnvelope<T>> {
    try { return await this.#callOnce<T>(method, params, "sdk-" + randomUUID(), options); }
    catch (error) {
      if (error instanceof TokenShrinkerTimeoutError ||
          error instanceof TokenShrinkerCancelledError ||
          error instanceof TokenShrinkerRemoteError) throw error;
      this.#discovery = undefined;
      return this.#callOnce<T>(method, params, "sdk-" + randomUUID(), options);
    }
  }
  async #callOnce<T extends object>(method: string, params: object, requestId: string,
    options: CallOptions): Promise<PublicEnvelope<T>> {
    const discovery = await this.#loadDiscovery();
    const timeoutMs = options.timeoutMs ?? 30_000;
    const request = { jsonrpc: "2.0", id: requestId,
      protocolVersion: { major: 1, minor: 0 }, authToken: discovery.authToken,
      method, params, deadlineUnixMs: Date.now() + timeoutMs };
    const response = await framedCall(discovery.endpoint, request, this.#maxFrameBytes,
      timeoutMs, options.signal, () => { void this.#cancel(discovery, requestId); });
    if (response.error) throw new TokenShrinkerRemoteError(response.error.code,
      response.error.dataCode ?? "remote-error", response.error.message);
    return response.result as PublicEnvelope<T>;
  }
  async #cancel(discovery: DiscoveryState, requestId: string): Promise<void> {
    const cancel = { jsonrpc: "2.0", id: "cancel-" + randomUUID(),
      protocolVersion: { major: 1, minor: 0 }, authToken: discovery.authToken,
      method: "daemon.cancel", params: { requestId }, deadlineUnixMs: Date.now() + 1_000 };
    try { await framedCall(discovery.endpoint, cancel, this.#maxFrameBytes, 1_000); } catch {}
  }
  async #loadDiscovery(): Promise<DiscoveryState> {
    this.#discovery ??= JSON.parse(await readFile(this.#discoveryPath, "utf8")) as DiscoveryState;
    if (this.#discovery.protocolVersion.major !==
        Number(DOMAIN_PROTOCOL_VERSION.split(".")[0])) {
      throw new TokenShrinkerError("Daemon protocol major is incompatible", "incompatible-protocol");
    }
    return this.#discovery;
  }
  public async close(): Promise<void> { this.#discovery = undefined; }
}

async function framedCall(endpoint: string, request: object, maxFrameBytes: number,
  timeoutMs: number, signal?: AbortSignal, onAbort?: () => void): Promise<RpcResponse> {
  return new Promise((resolve, reject) => {
    const socketEndpoint = process.platform === "win32" && !endpoint.startsWith("\\\\.\\pipe\\")
      ? "\\\\.\\pipe\\" + endpoint
      : endpoint;
    const socket = createConnection(socketEndpoint);
    const payload = Buffer.from(JSON.stringify(request));
    if (payload.length > maxFrameBytes) {
      socket.destroy(); reject(new TokenShrinkerError("Request frame is too large", "frame-too-large")); return;
    }
    const header = Buffer.alloc(4); header.writeUInt32BE(payload.length);
    let buffered = Buffer.alloc(0);
    const timer = setTimeout(() => {
      socket.destroy(); reject(new TokenShrinkerTimeoutError());
    }, timeoutMs);
    const cleanup = (): void => { clearTimeout(timer); signal?.removeEventListener("abort", abort); };
    const abort = (): void => {
      onAbort?.(); socket.destroy(); cleanup(); reject(new TokenShrinkerCancelledError());
    };
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) { abort(); return; }
    socket.on("connect", () => socket.write(Buffer.concat([header, payload])));
    socket.on("data", (chunk: Buffer) => {
      buffered = Buffer.concat([buffered, chunk]);
      if (buffered.length < 4) return;
      const length = buffered.readUInt32BE(0);
      if (length > maxFrameBytes) {
        socket.destroy(); cleanup();
        reject(new TokenShrinkerError("Response frame is too large", "frame-too-large")); return;
      }
      if (buffered.length >= 4 + length) {
        socket.end(); cleanup();
        try { resolve(JSON.parse(buffered.subarray(4, 4 + length).toString("utf8")) as RpcResponse); }
        catch (error) { reject(new TokenShrinkerError("Daemon returned invalid JSON", "invalid-json", error)); }
      }
    });
    socket.on("error", (error) => {
      cleanup(); reject(new TokenShrinkerError("Daemon connection failed", "connection", error));
    });
  });
}

export interface StdioTransportOptions { binaryPath?: string }
export class StdioTransport implements TokenShrinkerTransport {
  readonly #child: ChildProcessWithoutNullStreams;
  readonly #lines: Interface;
  readonly #pending = new Map<string | number, {
    resolve(value: unknown): void; reject(error: unknown): void;
  }>();
  #nextId = 1;
  private constructor(binaryPath: string) {
    this.#child = spawn(binaryPath, ["start", "--stdio"],
      { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
    this.#child.stderr.resume();
    const failPending = (error: unknown): void => {
      for (const pending of this.#pending.values()) pending.reject(error);
      this.#pending.clear();
    };
    this.#child.on("error", (error) => failPending(
      new TokenShrinkerError("MCP stdio process failed", "stdio-process", error)));
    this.#child.on("exit", (code) => {
      if (code !== 0) failPending(new TokenShrinkerError(
        "MCP stdio process exited unexpectedly", "stdio-exit"));
    });
    this.#lines = createInterface({ input: this.#child.stdout });
    this.#lines.on("line", (line) => {
      const message = JSON.parse(line) as { id?: string | number; result?: unknown;
        error?: { code: number; message: string } };
      if (message.id === undefined) return;
      const pending = this.#pending.get(message.id); if (!pending) return;
      this.#pending.delete(message.id);
      if (message.error) pending.reject(new TokenShrinkerRemoteError(
        message.error.code, "mcp-error", message.error.message));
      else pending.resolve(message.result);
    });
  }
  public static async connect(options: StdioTransportOptions = {}): Promise<StdioTransport> {
    const transport = new StdioTransport(options.binaryPath ??
      process.env.TOKEN_SHRINKER_BINARY ?? "token-shrinker");
    await transport.#request("initialize", { protocolVersion: MCP_PROTOCOL_VERSION,
      capabilities: {}, clientInfo: { name: "@token-shrinker/sdk", version: "0.1.2" } });
    transport.#notify("notifications/initialized", {}); return transport;
  }
  public async call<T extends object>(method: string, params: object,
    options: CallOptions = {}): Promise<PublicEnvelope<T>> {
    const result = await this.#request("tools/call", { name: method, arguments: params },
      options) as { structuredContent?: PublicEnvelope<T>; isError?: boolean;
        content?: Array<{ text?: string }> };
    if (result.isError || !result.structuredContent) {
      throw new TokenShrinkerError(result.content?.[0]?.text ?? "MCP tool failed", "tool-error");
    }
    return result.structuredContent;
  }
  #request(method: string, params: object, options: CallOptions = {}): Promise<unknown> {
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id); reject(new TokenShrinkerTimeoutError());
      }, options.timeoutMs ?? 30_000);
      const abort = (): void => {
        clearTimeout(timer); this.#pending.delete(id); reject(new TokenShrinkerCancelledError());
      };
      options.signal?.addEventListener("abort", abort, { once: true });
      this.#pending.set(id, { resolve: (value) => {
        clearTimeout(timer); options.signal?.removeEventListener("abort", abort); resolve(value);
      }, reject: (error) => {
        clearTimeout(timer); options.signal?.removeEventListener("abort", abort); reject(error);
      } });
      this.#child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    });
  }
  #notify(method: string, params: object): void {
    this.#child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
  }
  public async close(): Promise<void> {
    this.#lines.close(); this.#child.stdin.end();
    await new Promise<void>((resolve) => {
      if (this.#child.exitCode !== null) resolve();
      else this.#child.once("exit", () => resolve());
    });
  }
}

export interface ConnectOptions {
  transport?: "auto" | "daemon" | "stdio"; discoveryPath?: string; binaryPath?: string;
}
export class TokenShrinkerClient {
  private constructor(readonly transport: TokenShrinkerTransport) {}
  public static async connect(options: ConnectOptions = {}): Promise<TokenShrinkerClient> {
    if (options.transport === "stdio") return new TokenShrinkerClient(
      await StdioTransport.connect(options.binaryPath ? { binaryPath: options.binaryPath } : {}));
    if (options.transport === "daemon") return new TokenShrinkerClient(
      new DaemonTransport(options.discoveryPath ? { discoveryPath: options.discoveryPath } : {}));
    try {
      const daemon = new DaemonTransport(
        options.discoveryPath ? { discoveryPath: options.discoveryPath } : {});
      await daemon.call("token_shrinker_capabilities", {}, { timeoutMs: 1_000 });
      return new TokenShrinkerClient(daemon);
    } catch {
      return new TokenShrinkerClient(await StdioTransport.connect(
        options.binaryPath ? { binaryPath: options.binaryPath } : {}));
    }
  }
  public capabilities(options?: CallOptions): Promise<PublicEnvelope<CapabilityReport>> {
    return this.transport.call("token_shrinker_capabilities", {}, options);
  }
  public buildContext(request: BuildContextRequest, options?: CallOptions):
    Promise<PublicEnvelope<Record<string, unknown>>> {
    return this.transport.call("token_shrinker_build_context", request, options);
  }
  public route(request: object, options?: CallOptions):
    Promise<PublicEnvelope<Record<string, unknown>>> {
    return this.transport.call("token_shrinker_route", request, options);
  }
  public formatFinal(request: FormatFinalRequest, options?: CallOptions):
    Promise<PublicEnvelope<Record<string, unknown>>> {
    return this.transport.call("token_shrinker_format_final", request, options);
  }
  public close(): Promise<void> { return this.transport.close(); }
}
function defaultDiscoveryPath(): string {
  if (process.env.TOKEN_SHRINKER_RUNTIME_DIR)
    return join(process.env.TOKEN_SHRINKER_RUNTIME_DIR, "daemon.json");
  if (process.platform === "win32") return join(process.env.LOCALAPPDATA ?? tmpdir(),
    "Token-Shrinker", "runtime", "daemon.json");
  return join(process.env.XDG_RUNTIME_DIR ?? join(homedir(), ".local", "run"),
    "token-shrinker", "daemon.json");
}
