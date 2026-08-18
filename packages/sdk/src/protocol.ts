/** Generated from Token-Shrinker's Rust public protocol metadata. Do not hand edit. */
export const DOMAIN_PROTOCOL_VERSION = "1.0" as const;
export const PUBLIC_SCHEMA_VERSION = 1 as const;
export const MCP_PROTOCOL_VERSION = "2025-11-25" as const;
export type ToolName =
  | "token_shrinker_capabilities"
  | "token_shrinker_route"
  | "token_shrinker_task_status"
  | "token_shrinker_task_update"
  | "token_shrinker_build_context"
  | "token_shrinker_fetch_source"
  | "token_shrinker_search_memory"
  | "token_shrinker_remember"
  | "token_shrinker_execute"
  | "token_shrinker_stats"
  | "token_shrinker_format_final";
export type RouteMode = "FAST" | "BUILD" | "DEEP";
export type OutputMode = "lite" | "full" | "ultra" | "wenyan-lite" | "wenyan-full" | "wenyan-ultra" | "off";
export interface PublicEnvelope<T extends object> {
  protocolVersion: string; requestId: string; warnings: string[]; data: T;
}
export interface CapabilityReport {
  binaryVersion: string; packageVersion: string; protocolVersion: string;
  mcpProtocolVersion: string; schemaVersion: number;
  health: "healthy" | "degraded" | "failed";
  capabilities: Array<{ id: string; provider: string; fallback?: string | null;
    health: "healthy" | "degraded" | "failed"; warningCode?: string | null }>;
  tools: ToolName[];
}
export interface BuildContextRequest { root: string; goal: string; budget: number }
export interface FormatFinalRequest { text: string; mode?: OutputMode; agent?: string; tool?: string }
export interface CallOptions { timeoutMs?: number; signal?: AbortSignal }
