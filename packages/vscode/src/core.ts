/** Pure workspace-trust and response helpers shared by the VS Code host and tests. */

export type WorkspaceOperation = "status" | "build-context" | "stats";

export interface TrustDecision {
  allowed: boolean;
  warningCode?: "workspace-untrusted";
}

/** Allows content-free status everywhere and gates workspace-derived operations on trust. */
export function authorizeWorkspaceOperation(
  operation: WorkspaceOperation,
  trusted: boolean,
): TrustDecision {
  if (operation === "status" || trusted) return { allowed: true };
  return { allowed: false, warningCode: "workspace-untrusted" };
}

/** Extracts a stable one-line health label from an SDK capability response. */
export function statusLabel(value: unknown): string {
  if (!isRecord(value)) return "Token-Shrinker: unavailable";
  const data = value.data;
  if (
    !isRecord(data) ||
    (data.health !== "healthy" && data.health !== "degraded" && data.health !== "failed")
  ) {
    return "Token-Shrinker: unavailable";
  }
  return `Token-Shrinker: ${data.health}`;
}

/** Serializes structured results for an editor document without interpreting provider output. */
export function renderStructuredResult(value: unknown): string {
  return JSON.stringify(value, null, 2) + "\n";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
