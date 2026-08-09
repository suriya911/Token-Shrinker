# Threat model

## Scope and security objectives

Token-Shrinker is a local tool/runtime layer between an agent and local context, memory, execution,
and optional-provider tools. It does not proxy or replace the agent's model transport. The primary
objectives are to preserve source integrity and provenance, prevent unauthorized reads/execution,
keep secrets and cross-repository memory out of outputs, and fail safely when optional components
are absent or hostile.

## Assets

- repository source, diagnostics, prompts, and generated context;
- user and repository-scoped SQLite memory;
- local IPC authentication tokens and daemon discovery state;
- execution approvals, exact command arrays, raw-output handles, and telemetry metadata;
- adapter configuration, update manifests, binaries, npm/VSIX packages, checksums, and SBOMs;
- native agent model-provider configuration and credentials.

## Actors

- the local user and an authorized editor/agent client;
- repository authors, including an attacker controlling cloned repository content;
- optional provider executables and MCP servers, which are not trusted;
- dependency, package-registry, CI, or release-channel attackers;
- another local process attempting IPC impersonation or data discovery.

## Trust boundaries

1. Agent/editor to versioned MCP or authenticated local IPC.
2. Untrusted repository paths/content to the canonical-root scanner and redaction policy.
3. Approved execution request to exact process/argument launch and bounded output capture.
4. Token-Shrinker to optional provider subprocesses with declared content boundaries.
5. Runtime to local SQLite, telemetry, cache, and raw-artifact directories.
6. Source repository to CI, package construction, signed update metadata, and user installation.
7. VS Code untrusted workspace to extension commands that could read repository content.

## Threats, mitigations, and verification

| Threat | Mitigation | Verification | Residual risk |
|---|---|---|---|
| Path traversal or symlink escape | Canonical allowed roots; no symlink following; encoded source handles | adversarial repository and fetch tests | filesystem races outside supported local threat model |
| Command/argument injection | Exact executable and argument arrays; no shell; workspace allowlist; approval boundary | execution-policy and provider fake-process tests | approved executable may itself interpret arguments unsafely |
| Malicious repository configuration | Typed configuration; repository layer cannot weaken user security policy | configuration fixture tests; VS Code trust tests | social engineering may persuade a user to trust a hostile workspace |
| Secret capture | Pre-cache redaction, content-free telemetry, synthetic canary, published-report exclusion | repository redaction and deterministic demo gates; tracked-file secret scan | unknown secret formats may evade pattern policy |
| Hostile terminal escapes | Raw bytes retained separately; structured summaries; editor opens JSON without command interpretation | terminal parser and output-profile tests | terminal rendering outside Token-Shrinker may interpret raw bytes |
| IPC impersonation | Per-user endpoint, random bounded auth token, framed size limits | authentication, stale-state, frame, and shutdown tests | a fully compromised same-user account can read user files |
| Compromised optional provider | Minimal explicit boundary, version/capability probes, deadlines, size/schema checks, circuit breaker, fallback | unavailable/timeout/crash/malformed/incompatible fake-provider suite | provider sees content explicitly sent to it |
| Malicious provider output | Treated as data; schema validation; never used as a shell command | MCP contract tests and fuzz corpus | semantic misinformation can still require evidence review |
| Cross-repository memory disclosure | Explicit user/repository scope and parameterized SQLite queries | scope, expiry, migration, and deletion tests | user-scoped memories are intentionally cross-repository |
| Denial of service | Frame/file/output/token/time/concurrency limits and cancellation | limit, timeout, backpressure, and circuit tests | local resource exhaustion can still reduce availability |
| Dependency or registry compromise | Exact lockfiles, minimum release age, disabled unneeded install scripts, advisory/license scans, CodeQL, SBOM | CI security workflow | trusted upstream release or CI identity can be compromised |
| Malicious update/downgrade | Signed expiring manifest, checksum, source binding, anti-downgrade and compatibility rules; check-only default | update resolver fixtures and native-transport regression tests | production signing ceremony remains a release responsibility |
| Native model-transport regression | Adapters use MCP only and never mutate provider base URLs | before/after adapter fixtures and `doctor` diagnostics | unrelated user tooling may alter those variables |
| VS Code workspace abuse | `limited` untrusted-workspace capability; context/stats blocked until trusted | pure trust-policy tests and VSIX manifest inspection | health command still launches the configured local binary |

## Data locations, retention, and deletion

The runtime and data directories are reported by `token-shrinker doctor --json`. SQLite memory and
telemetry remain local; content telemetry is disabled. Raw artifacts use a bounded count and TTL.
`memory clear`, cache operations, adapter uninstall, and removal of the reported Token-Shrinker data
directory provide user-controlled deletion. Filesystem and SSD behavior means deletion cannot
promise forensic erasure. Backups and external indexing are outside the runtime's control.

## Network and credential boundaries

The baseline runtime makes no model or hosted-telemetry request. Update checks access only the
configured authoritative manifest source. Optional providers may perform network operations under
their own configuration; `doctor` states the content boundary before use. Token-Shrinker does not
collect provider credentials and never rewrites `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, or other
native model endpoints.

## Review status

The automated M7 review covers dependency/advisory/license scans, CodeQL, secret canaries, SBOM,
parser corpus, clean packaging, update fixtures, native transport, and the public demo. Residual
risks above require normal release review and responsible disclosure through `SECURITY.md`.
