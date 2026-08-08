# Token-Shrinker: Implementation-Ready Build Plan

This plan takes Token-Shrinker from an empty repository to a production-ready `v1.0.0`. It is ordered so Codex or a human team can implement one verifiable slice at a time without depending on optional third-party tools.

## 1. Product contract

### Goal

Build a universal, local-first token-optimization runtime for AI/LLM CLI agents. It must select an optimization strategy, assemble relevant context inside a token budget, compress execution output, expose local telemetry, and integrate through MCP, a CLI, and a TypeScript SDK.

### Non-goals for v1

- replacing the agent or choosing its model;
- compressing or exposing private chain-of-thought;
- executing arbitrary commands without an explicit request and policy approval;
- requiring Graphify, Headroom, RTK, Claude-Mem, or any hosted service;
- promising a token reduction without measuring evidence retention and task outcome;
- maintaining private patches against agent internals.

### Release invariants

1. The Rust core owns routing, ranking, execution, telemetry, and policy.
2. MCP/IPC schemas are versioned and language-neutral.
3. TypeScript clients contain transport and integration logic, not a second optimizer.
4. Built-in providers always support the core workflow.
5. Optional providers are detected at runtime and fail open to a safe fallback, unless explicitly configured as required.
6. Context is provenance-rich: every included segment can be traced to a source and range.
7. Telemetry is local and content-free by default.
8. Concise/Caveman-style output is a final-response formatting option only.
9. Caveman `full` is the default human-output mode; every agent and tool supports scoped overrides without changing raw evidence or machine output.
10. Agent-to-model traffic remains on the agent's native provider transport by default; Token-Shrinker never globally replaces provider base URLs.
11. Late features remain optional behind versioned contracts, migrations, capability flags, and rollback tests.
12. Version 1 performs signed, read-only compatibility checks; any later managed updater requires a separate readiness review and never silently elevates privileges.

## 2. Architecture decisions to record before coding

Create these architecture decision records under `docs/adr/` in M0:

| ADR | Decision | Reason |
|---|---|---|
| 0001 | Rust core plus thin TypeScript edge | predictable latency with broad npm/editor compatibility |
| 0002 | MCP first; versioned JSON-RPC for local SDK IPC | interoperable agent surface and debuggable transport |
| 0003 | stdio and one-shot first; optional daemon after measured need | validate the core before adding lifecycle complexity |
| 0004 | deterministic explainable router in v1 | testability, low overhead, no model dependency |
| 0005 | SQLite built-in memory and telemetry | portable, transactional, local-first persistence |
| 0006 | provider traits with capability negotiation | optional integrations and graceful degradation |
| 0007 | exact tokenizers when known; labeled conservative estimate otherwise | honest cross-model accounting |
| 0008 | extractive compression before abstractive compression | preserve evidence and provenance by default |
| 0009 | explicit execution authorization and argument-array process launch | reduce command-injection and surprise-execution risk |
| 0010 | npm platform packages without an alpha downloader | reliable, offline-capable distribution with a smaller supply-chain surface |
| 0011 | native model transport is an invariant; Claude/Headroom uses MCP, not `ANTHROPIC_BASE_URL` proxying | preserve `/remote-control`, native context limits, authentication, and vendor features |
| 0012 | signed read-only v1 update checks; transactional updates post-v1 | ship the optimizer before accepting self-update risk |
| 0013 | additive extension model with capability flags and migration/rollback gates | allow late features without destabilizing existing agents |

Do not start public API implementation until ADRs 0001-0013 and protocol terminology have been reviewed together.

## 3. Target repository

```text
token-shrinker/
|-- .cargo/config.toml
|-- .config/nextest.toml
|-- .github/
|   |-- ISSUE_TEMPLATE/
|   |-- dependabot.yml
|   `-- workflows/
|       |-- ci.yml
|       |-- benchmark.yml
|       |-- codeql.yml
|       |-- upstream-watch.yml
|       |-- compatibility.yml
|       |-- publish-update-manifest.yml
|       `-- release.yml
|-- crates/
|   |-- ts-types/
|   |-- ts-config/
|   |-- ts-router/
|   |-- ts-context/
|   |-- ts-memory/
|   |-- ts-repo/
|   |-- ts-compress/
|   |-- ts-output/
|   |-- ts-exec/
|   |-- ts-telemetry/
|   |-- ts-update/
|   |-- ts-protocol/
|   |-- ts-daemon/
|   |-- ts-mcp/
|   `-- ts-cli/
|-- packages/
|   |-- sdk/
|   |-- cli/
|   |-- adapter-core/
|   |-- adapter-claude-code/
|   |-- adapter-codex/
|   |-- adapter-gemini/
|   |-- adapter-opencode/
|   |-- adapter-aider/
|   `-- vscode/
|-- skills/token-shrinker/SKILL.md
|-- schemas/
|-- fixtures/
|   |-- demo-repo/
|   |-- routing/
|   |-- context-ranking/
|   `-- terminal-output/
|-- benchmarks/
|   |-- suites/
|   |-- baselines/
|   `-- reports/
|-- docs/
|   |-- adr/
|   |-- adapters/
|   |-- protocol/
|   `-- threat-model.md
|-- examples/
|-- scripts/
|-- Cargo.toml
|-- Cargo.lock
|-- rust-toolchain.toml
|-- package.json
|-- pnpm-lock.yaml
|-- pnpm-workspace.yaml
|-- README.md
|-- BUILD_PLAN.md
|-- CONTRIBUTING.md
|-- SECURITY.md
|-- CODE_OF_CONDUCT.md
`-- LICENSE
```

Keep crate dependency flow acyclic:

```text
types <- config
types <- router
types <- context <- repo
types <- context <- memory
types <- compress <- exec
types <- telemetry
all domain crates <- daemon <- protocol/MCP/CLI
config/protocol/types <- update <- daemon/CLI
protocol-generated types <- TypeScript SDK <- adapters/VS Code
```

`ts-types` must not depend on infrastructure crates. Provider traits should live beside the domain that owns them, not in a generic plugin crate.

## 4. Stable domain contracts

The exact Rust syntax may evolve during M1, but these semantics must remain stable through the alpha.

### Core request and response models

```rust
pub enum RouteMode { Auto, Fast, Build, Deep }

pub struct OptimizeRequest {
    pub request_id: RequestId,
    pub goal: String,
    pub mode: RouteMode,
    pub budget: TokenBudget,
    pub sources: Vec<SourceSpec>,
    pub execution: Option<ExecutionRequest>,
    pub final_output: FinalOutputMode,
}

pub struct TokenBudget {
    pub max_input_tokens: u32,
    pub reserved_output_tokens: u32,
    pub tokenizer: Option<TokenizerId>,
}

pub struct OptimizeResponse {
    pub protocol_version: ProtocolVersion,
    pub route: RouteDecision,
    pub context: ContextBundle,
    pub execution: Option<ExecutionResult>,
    pub metrics: RequestMetrics,
    pub warnings: Vec<Warning>,
}

pub struct RouteDecision {
    pub mode: RouteMode,
    pub reasons: Vec<RouteReason>,
    pub effective_budget: TokenBudget,
}
```

### Provider traits

```rust
#[async_trait]
pub trait ContextProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn health(&self) -> Health;
    async fn candidates(&self, query: &ContextQuery)
        -> Result<Vec<ContextCandidate>, ProviderError>;
    async fn fetch(&self, source: &SourceRef)
        -> Result<SourceDocument, ProviderError>;
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn search(&self, query: &MemoryQuery)
        -> Result<Vec<MemoryRecord>, ProviderError>;
    async fn put(&self, record: NewMemoryRecord)
        -> Result<MemoryId, ProviderError>;
    async fn forget(&self, selector: ForgetSelector)
        -> Result<ForgetReport, ProviderError>;
}

#[async_trait]
pub trait Compressor: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn compress(&self, input: CompressionInput, budget: TokenBudget)
        -> Result<CompressedArtifact, ProviderError>;
}

#[async_trait]
pub trait TokenCounter: Send + Sync {
    fn descriptor(&self) -> TokenizerDescriptor;
    async fn count(&self, text: &str) -> Result<TokenCount, TokenError>;
}

#[async_trait]
pub trait ExecutionPolicy: Send + Sync {
    async fn authorize(&self, request: &ExecutionRequest)
        -> Result<Authorization, PolicyError>;
}

#[async_trait]
pub trait ManagedTool: Send + Sync {
    fn descriptor(&self) -> ManagedToolDescriptor;
    async fn detect(&self) -> Result<InstalledTool, UpdateError>;
    async fn plan(&self, target: &ReleaseTarget) -> Result<ToolUpdatePlan, UpdateError>;
    async fn stage(&self, plan: &ToolUpdatePlan) -> Result<StagedTool, UpdateError>;
    async fn activate(&self, staged: &StagedTool) -> Result<ActivationReceipt, UpdateError>;
    async fn verify(&self, receipt: &ActivationReceipt) -> Result<Health, UpdateError>;
    async fn rollback(&self, receipt: &ActivationReceipt) -> Result<(), UpdateError>;
}

pub enum CavemanMode {
    Lite,
    Full,
    Ultra,
    WenyanLite,
    WenyanFull,
    WenyanUltra,
    Off,
}

pub struct OutputPolicy {
    pub default: CavemanMode,
    pub agent_overrides: BTreeMap<AgentId, CavemanMode>,
    pub tool_overrides: BTreeMap<ToolId, CavemanMode>,
    pub auto_clarity: bool,
    pub preserve: PreservationPolicy,
}

pub trait OutputFormatter: Send + Sync {
    fn format(&self, input: UserFacingArtifact, policy: &ResolvedOutputPolicy)
        -> Result<FormattedArtifact, FormatError>;
}
```

### Candidate and provenance model

```rust
pub struct ContextCandidate {
    pub source: SourceRef,
    pub span: Option<SourceSpan>,
    pub content: String,
    pub content_hash: ContentHash,
    pub estimated_tokens: u32,
    pub signals: RelevanceSignals,
    pub sensitivity: Sensitivity,
    pub observed_at: Timestamp,
}

pub struct RankedContext {
    pub candidate: ContextCandidate,
    pub score: f32,
    pub score_breakdown: Vec<ScoreComponent>,
    pub disposition: Disposition, // Included, Duplicate, OverBudget, PolicyDenied
}

pub struct ContextBundle {
    pub text: String,
    pub citations: Vec<Citation>,
    pub included: Vec<SourceRef>,
    pub omitted: Vec<Omission>,
    pub token_count: TokenCount,
    pub bundle_hash: ContentHash,
}
```

Candidate scoring should be a documented weighted function in v1:

```text
score = goal_similarity
      + symbol_or_path_match
      + graph_proximity
      + recency
      + diagnostic_priority
      + user_pin
      - duplication
      - staleness
      - token_cost_penalty
```

Normalize components, record the score breakdown, and use deterministic tie-breaking by canonical source ID and span. Start with tested heuristic weights; learnable ranking is post-v1.

### Capabilities and degradation

```rust
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub version: String,
    pub capabilities: BTreeSet<Capability>,
    pub availability: Availability,
    pub data_boundary: DataBoundary,
}

pub enum Availability {
    Available,
    Degraded { fallback: ProviderId, reason: String },
    Unavailable { reason: String },
}
```

Registration rule:

1. Load provider configuration.
2. Probe executable/socket/protocol with a short timeout.
3. Validate a compatible version or feature set.
4. Register the optional provider if healthy.
5. Otherwise register the built-in fallback and a structured warning.
6. Fail startup only when the optional provider is explicitly `required = true`.

Never silently route data to a network provider as a fallback.

## 5. Router specification

Implement the router without an LLM. Inputs are goal features, requested operations, repository breadth, candidate count, explicit mode, and budget.

Initial decision rules, in priority order:

1. An explicit `fast`, `build`, or `deep` request wins if allowed by configured limits.
2. Architecture/repository-wide terms, more than three named subsystems, or an estimated scan above the deep threshold select `DEEP`.
3. Requested edits, tests, builds, debugging, or multi-file context select `BUILD`.
4. A focused lookup, single known path/symbol, or metadata request selects `FAST`.
5. Ambiguity defaults to `BUILD`, not `DEEP`.

Return reason codes such as `explicit_override`, `execution_requested`, `cross_cutting_scope`, and `single_symbol_lookup`. Unit tests must use table-driven fixtures with no wall-clock or filesystem dependency.

## 6. Context pipeline specification

Run the following stages with per-stage timings:

1. Parse the goal and explicit source hints.
2. Query eligible providers concurrently with bounded fan-out and deadlines.
3. Normalize source IDs and line/symbol spans.
4. Apply allowed-root policy and secret redaction.
5. Hash and deduplicate exact content.
6. Collapse overlapping spans and repeated generated files.
7. Count exact tokens when supported; otherwise use a conservative estimate.
8. Score every candidate and retain the score breakdown.
9. Pack candidates into the budget using priority, marginal usefulness, and token cost.
10. Apply extractive compression to oversized candidates while retaining citations.
11. Reserve a small provenance/omission budget.
12. Emit a canonical context bundle and metrics event.

Packing v1 can use deterministic greedy selection after mandatory pinned/diagnostic evidence. Add a benchmark before replacing it with knapsack or learned ranking.

Built-in repository support must include:

- ignore-file handling (`.gitignore`, project exclusions, generated/vendor defaults);
- file metadata and bounded content reads;
- text/path search;
- language and likely-generated-file detection;
- optional tree-sitter symbol extraction behind a feature flag;
- content-hash cache invalidation;
- binary and oversized-file rejection with structured omissions.

Graphify is a `ContextProvider`, not a required indexer. It adds graph-neighbor and symbol-edge candidates; the native provider remains available.

## 7. Compression specification

### Built-in context compression

- remove duplicate passages and boilerplate;
- keep matched lines with configurable surrounding context;
- preserve headings, source spans, diagnostics, and definition signatures;
- never rewrite source code in a way that looks verbatim;
- expose omitted ranges so an agent can fetch them.

### Built-in terminal compression

Parse output as a stream and preserve:

- command, exit code, duration, truncation state;
- first occurrence and count of unique errors/warnings;
- failing test names, assertion differences, file/line references;
- bounded first/tail output;
- repeated-line counts and collapsed progress noise;
- an opaque/raw-output handle with retention limits.

RTK is an optional terminal `Compressor`. Headroom is an optional context `Compressor`. Each adapter must translate into the same `CompressedArtifact`, annotate its provider/version, enforce time/output limits, and fall back on timeout, incompatible output, nonzero exit, or unavailable executable.

Caveman formatting receives only approved user-facing artifacts. It may shorten prose and formatting but must preserve code, paths, commands, warnings, citations, and uncertainty. It must not operate on hidden reasoning or raw credentials.

### Caveman profile engine

Support exact modes `lite`, `full`, `ultra`, `wenyan-lite`, `wenyan-full`, `wenyan-ultra`, and `off`. Global default is `full`.

Resolve mode in this order: request, session, tool, agent, global. Built-in tool IDs are stable and include `router`, `context`, `memory`, `graphify`, `headroom`, `rtk`, `execution`, `stats`, `doctor`, and `update`. Future tools receive `full` automatically until configured.

Formatting occurs only at a human-output boundary after normalization. Never format JSON/CSV, MCP payloads, source code, raw retained logs, internal context, prompts, reasoning, database records, signatures, manifests, third-party messages, or test golden inputs. Preserve exact code, commands, paths, API names, errors, numbers, units, citations, uncertainty, negation, exclusions, and permission/security meaning.

With `auto_clarity`, security warnings, irreversible-action confirmations, ordered migration/recovery steps, and ambiguous compressed results use `lite` or `off` for that artifact only. Record requested/resolved mode and clarity reason in content-free telemetry.

Add golden tests for every mode and language boundary. Add semantic preservation tests that compare extracted commands, paths, citations, numbers, error strings, negations, and required facts before/after formatting. Compression target never overrides semantic correctness.

## 8. Memory and telemetry

### SQLite schema

Use migrations from the first commit. Suggested tables:

```text
schema_migrations(version, applied_at)
memories(id, scope, kind, content, content_hash, source, created_at, expires_at)
memory_tags(memory_id, tag)
memory_fts(memory_id, content)                  # SQLite FTS5 when available
requests(id, session_id, agent, mode, started_at, duration_ms, status)
token_events(id, request_id, stage, direction, raw_tokens, optimized_tokens,
             tokenizer, exact, created_at)
provider_events(id, request_id, provider, operation, duration_ms, status, warning_code)
artifacts(id, request_id, kind, content_hash, byte_count, expires_at)
```

Separate memory from ephemeral raw-output artifacts. Telemetry rows must never contain content. Provide retention and deletion transactions plus `memory forget`, `cache prune`, and a full local-data purge command that previews exact paths and record counts before confirmation.

Optional Claude-Mem and generic MCP memory adapters implement `MemoryProvider`. Validate provider identity and protocol version. Namespace records by repository/user scope and do not mirror external contents into SQLite unless caching is enabled explicitly.

### Token accounting

Record these values independently:

- source/raw candidate tokens;
- selected context tokens;
- compressed execution tokens;
- protocol overhead tokens when measurable;
- reserved output budget;
- exact-versus-estimated flag and tokenizer ID.

Define savings as:

```text
savings_tokens = comparable_raw_tokens - optimized_tokens
savings_percent = savings_tokens / comparable_raw_tokens * 100
```

Do not combine incomparable model tokenizers in one aggregate without labeling and normalization.

## 9. Daemon, IPC, and lifecycle

### Native transport and Remote Control invariant

The daemon serves tools and context; it does not proxy the agent's model API. The default adapters must not set or rewrite `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, equivalent provider endpoints, or model credentials.

Claude Code uses Token-Shrinker and Headroom through MCP so `/remote-control` (`/rc`) and other native features remain on Claude's direct Anthropic connection. If an integration offers transport wrapping for a different agent, it must be explicit, scoped to that adapter, reversible, and covered by a compatibility fixture. No updater, migration, late feature, or generated shell wrapper may convert Claude to proxy mode.

`doctor` must detect provider-base-URL overrides and wrapper recursion, explain their effect, and provide remediation without changing them automatically. Adapter acceptance tests snapshot relevant environment/configuration before and after install/update and fail if native transport changes unexpectedly.

### Runtime modes

1. `token-shrinker start --stdio`: dedicated MCP server, lifecycle owned by the client.
2. `token-shrinker start`: per-user background daemon for SDK/CLI/editor reuse.
3. one-shot: CLI runs the service graph in-process when daemon startup is disabled or fails safely.

### Local transport

- Unix: per-user Unix domain socket under the platform runtime directory, mode `0600`.
- Windows: per-user named pipe protected by the current user's ACL.
- Protocol: JSON-RPC 2.0 messages with explicit `protocolVersion`, bounded frame size, request IDs, cancellation, deadlines, and structured errors.
- Discovery: a user-only state file containing endpoint, PID, start time, protocol version, and ephemeral authentication material where required.

Do not use an unauthenticated localhost TCP port by default. The daemon must reject incompatible major versions and negotiate optional minor-version features.

### Concurrency and shutdown

- one cancellation token per request;
- bounded worker pools for indexing and processes;
- per-provider semaphore and deadline;
- backpressure for streamed output;
- graceful drain with a fixed timeout;
- stale endpoint/PID recovery that verifies process identity before cleanup;
- single-instance locking per user and protocol major version.

## 10. MCP and CLI contract

### MCP tools

Implement in this order:

1. `token_shrinker_capabilities`
2. `token_shrinker_route`
3. `token_shrinker_build_context`
4. `token_shrinker_fetch_source`
5. `token_shrinker_stats`
6. `token_shrinker_search_memory`
7. `token_shrinker_remember`
8. `token_shrinker_execute`
9. `token_shrinker_format_final`

Read-only tools should be marked read-only in MCP annotations. Mutating/execution tools must accurately declare side effects. Validate every request against generated schemas, cap strings/lists/frame sizes, and return stable error codes.

### Required CLI behavior

#### `token-shrinker init`

- detect repository and user configuration locations;
- show the files it will create;
- create minimal versioned config and database migrations;
- never overwrite existing config without confirmation or `--force`;
- support `--dry-run` and `--json`.

#### `token-shrinker doctor`

- inspect platform/architecture, config, data paths, daemon, protocol, database, agent adapters, optional providers, execution policy, and tokenizer availability;
- print `healthy`, `degraded`, or `failed` per capability;
- identify the active fallback and remediation;
- return nonzero only for required capability failures;
- redact secrets and support stable JSON.

#### `token-shrinker start`

- support background, foreground, and stdio modes;
- enforce single-instance policy;
- provide readiness and structured startup errors;
- log locally with rotation and no content bodies by default.

#### `token-shrinker stats`

- filter by date/session/agent/mode/provider;
- show raw, optimized, and saved tokens plus latency percentiles;
- identify estimates and mixed tokenizers;
- export JSON/CSV without content.

#### `token-shrinker add <integration>`

- support `--dry-run` and print the exact target file;
- back up or transactionally edit only the integration-owned block;
- validate after write and roll back on failure;
- never overwrite unrelated user configuration;
- make repeated calls idempotent.

#### `token-shrinker update --check`

- read and verify a signed, non-expired compatibility manifest;
- interpret `latest` as the latest compatible stable release from each tool's declared authoritative source;
- support `--json`, `--tool`, and `--channel` filters;
- detect ownership such as npm, Cargo, external manager, or manual installation;
- report exact old/new versions, sources, compatibility reasoning, and required package-manager commands;
- reject downgrade, identity mismatch, incompatible, expired, unsigned, or tampered metadata;
- never download, install, activate, restart, or replace components in version 1.

Transactional activation, rollback, schedulers, and unattended application are post-v1. They require a separate readiness review. If automation is introduced, its initial default is `notify`.

#### `token-shrinker output get|set`

- expose global, agent, tool, and session scopes;
- default to `full` for all unconfigured scopes and newly discovered tools;
- validate only supported Caveman modes;
- support `--json` without applying Caveman formatting to JSON;
- show effective value plus source scope;
- allow `--tool`, `--agent`, `--session`, `--reset`, and `--auto-clarity`;
- update config transactionally and preserve unrelated settings.

## 11. Update and extension architecture

Version 1 implements only signed manifest verification and read-only compatibility reporting. The transaction, scheduler, managed-store, and upstream-automation subsections below are a post-v1 reference design and are not v1 acceptance requirements.

### Compatibility manifest

Publish a signed, versioned manifest from the Token-Shrinker release repository. It must contain:

```text
manifest schema/version and expiry
component/tool ID and authoritative source
release version, channel, platform, and artifact identity
checksums, signatures/provenance, and SBOM reference
supported Token-Shrinker protocol/schema ranges
supported agent and optional-provider version ranges
required migrations and rollback compatibility
known conflicts, feature flags, and minimum OS/runtime
```

The update checker must reject unknown manifest majors, expired metadata outside a small documented grace policy, downgrade attacks, checksum/signature mismatch, incompatible dependency sets, and a component whose source does not match its registered tool ID. Cache the last verified manifest for offline diagnosis, never for indefinite trust.

### Scalable open-source tool registry

Each supported product is described by a reviewed manifest under `tools/<tool-id>/tool.toml` plus its adapter tests. A descriptor declares release discovery, artifact naming, supported platforms, version extraction, health probe, protocol ranges, data boundary, install layout, and rollback behavior. The update engine consumes the generic descriptor/trait contract; adding a conforming tool does not require changing its resolver or transaction state machine.

Third-party contributors add or change support through pull requests with fixtures and compatibility tests. Code-owning maintainers review executable update hooks and source changes. Manifests cannot embed arbitrary shell commands; operations use typed, allowlisted primitives or audited adapter code.

### Daily upstream compatibility pipeline

Run the following protected workflow every day and on manual dispatch:

1. Read all tool descriptors and last published compatibility state.
2. Query authoritative upstream APIs/registries with conditional requests and pinned identities.
3. If no upstream version changed, emit a small `no_change` result and stop without publishing.
4. For each changed product, create a candidate lockfile containing exact versions and artifact digests.
5. Run resolver, signature/checksum, license, SBOM, malware/static, and provenance checks.
6. Test the candidate matrix on Windows, macOS, and Linux against supported Token-Shrinker and agent versions.
7. Run provider contract failures, database migrations, adapter install/update/uninstall, native provider endpoint, Claude `/rc` eligibility, and benchmark regression gates.
8. Quarantine failures and publish their CI evidence without changing the client manifest.
9. Merge passing candidates into a generated compatibility lock through protected review/automation.
10. Sign and publish a short-lived compatibility manifest only after every referenced artifact is independently available and verified.
11. Clients discover the manifest during their next check and notify by default; post-v1 activation occurs only when separately enabled.

Use least-privilege OIDC/trusted publishing, protected environments, reproducible workflow definitions, immutable action pins, artifact attestations, and threshold approval for signing-key changes. A compromised upstream release must not bypass compatibility CI merely by reporting a higher version.

### Update transaction state machine

```text
DISCOVER -> RESOLVE -> PLAN -> STAGE -> VERIFY_STAGE
         -> WAIT_FOR_IDLE -> SNAPSHOT -> DRAIN -> ACTIVATE
         -> MIGRATE -> RESTART -> HEALTH_CHECK -> COMMIT
                                             \-> ROLLBACK
```

Persist the state before each transition so an interrupted update can resume verification or roll back. Component activation must be atomic where the platform permits it (versioned directory plus pointer/launcher switch). Database migrations must declare whether rollback is supported; otherwise make a verified database backup and restore it on transaction failure.

Do not self-replace a running executable in place on Windows. Launch a minimal signed updater helper from staging, verify its identity, exit/drain the daemon, switch versioned paths, then restart and health-check. Unix/macOS use the same versioned-path design for consistent behavior.

### External package managers

Each tool adapter declares ownership: `token_shrinker_managed`, `npm`, `cargo`, `pipx`, `brew`, `winget`, `manual`, or `unknown`. Token-Shrinker may execute an external manager only with explicit consent and after showing the command/source; it must not use `sudo`, UAC elevation, or global writes silently. If safe automation is unavailable, the one-command transaction reports `action_required` and leaves the current working version active.

If a user explicitly enables post-v1 managed activation, setup may install supported optional products into `token_shrinker_managed` user scope or register an existing managed copy. External ownership exists for compatibility and discovery, but it is never eligible for replacement until the user deliberately imports that tool into the managed store.

### Late-feature compatibility rules

- add capability flags before depending on new behavior;
- make protocol-minor fields optional with safe defaults;
- keep older adapters operational for the documented compatibility window;
- put experimental features behind opt-in flags and separate telemetry labels;
- ship forward migrations, fixtures from every supported version, and recovery tests;
- never turn an optional provider into a startup requirement during a minor release;
- preserve native agent transport and execution policy across all migrations;
- require an ADR and major version for incompatible public contract changes.

## 12. TypeScript layer

### `@token-shrinker/sdk`

Support Node.js active LTS. Provide:

- `TokenShrinkerClient.connect(options)`;
- typed methods matching MCP/domain operations;
- daemon discovery and optional auto-start;
- request deadlines and `AbortSignal` cancellation;
- reconnect with bounded retry only for idempotent operations;
- stable error subclasses mapped from protocol codes;
- ESM as the primary build and documented CommonJS compatibility policy;
- no postinstall network requirement for the SDK itself.

### `@token-shrinker/cli`

Use a tiny JavaScript launcher to select a platform package, verify the binary exists, and forward stdio/signals/exit codes. Do not reimplement commands in JavaScript.

Planned optional platform packages:

```text
@token-shrinker/win32-x64
@token-shrinker/win32-arm64
@token-shrinker/darwin-x64
@token-shrinker/darwin-arm64
@token-shrinker/linux-x64-gnu
@token-shrinker/linux-arm64-gnu
```

Add musl packages only after CI runners and smoke tests exist. Alpha packages must not download executables at install time or runtime. A future download fallback requires a separate security review and ADR amendment.

### Adapter core

```ts
export interface AgentAdapter {
  readonly id: string;
  detect(): Promise<DetectionResult>;
  planInstall(options: InstallOptions): Promise<InstallPlan>;
  install(plan: InstallPlan): Promise<InstallResult>;
  validate(): Promise<ValidationResult>;
  uninstall(options: UninstallOptions): Promise<UninstallResult>;
}
```

An install plan lists exact files, before/after owned fragments, commands, and rollback steps. Adapters must be idempotent and fixture-tested on Windows/macOS/Linux path conventions. Each adapter must cite the public integration mechanism and pin a tested compatibility range.

### VS Code extension

Keep the extension thin:

- start/connect/status commands;
- `doctor` diagnostics panel;
- repository enable/disable control;
- local savings view;
- capability/fallback status;
- links to configuration and logs;
- no source upload and no separate optimizer.

Use the daemon through the SDK. Add workspace-trust handling and never execute or index an untrusted workspace until the user enables it.

## 13. Step-by-step implementation milestones

Each milestone ends with a runnable artifact and a gate. Do not start dependent milestone work when a gate is red.

### M0 — Empty repo to governed skeleton

Tasks:

1. Initialize Git, Rust workspace, pnpm workspace, Node/Rust version pins, editor settings, and ignore files.
2. Add README, this plan, Apache-2.0 license metadata, contributing guide, code of conduct, security policy, changelog, and maintainer placeholders.
3. Create all crate/package directories with minimal compileable exports.
4. Add ADR template and ADRs 0001-0013.
5. Add CI for formatting, lint, compile, unit tests, and documentation on all three OS families.
6. Add conventional commit/release policy and dependency update automation.
7. Create a deterministic demo repository and fixture manifest before optimization code.
8. Add a repository command such as `pnpm check` that runs all local gates.
9. Add compatibility-manifest schemas and an initial native-transport regression fixture.
10. Add the signed compatibility-manifest schema and deterministic tampered/expired/incompatible fixtures for the read-only update checker.

Gate:

- clean clone passes `cargo test --workspace` and `pnpm install && pnpm build && pnpm test`;
- governance/security files exist;
- fixture content and expected relevant evidence are committed;
- CI branch protection requirements are documented.

### M1 — Types, configuration, router, and token accounting

Tasks:

1. Implement IDs, errors, budgets, route modes, request/response models, citations, warnings, and serialization snapshots.
2. Implement layered config with schema versioning, validation, safe defaults, and repository/user security precedence.
3. Implement deterministic router rules and reason codes.
4. Implement exact tokenizer provider(s) where redistribution permits and conservative fallback estimation.
5. Add property tests for nonnegative counts, budget bounds, deterministic routing, and serialization round trips.
6. Generate JSON Schema and TypeScript types; fail CI on schema drift.
7. Add a microbenchmark for route selection and token counting.
8. Implement `ts-output`, Caveman mode parsing, scope resolution, `full` defaults, preservation policy, auto-clarity classification, and golden/semantic tests.

Gate:

- every routing fixture has the expected mode and reasons;
- unknown config keys produce actionable diagnostics according to the compatibility policy;
- estimates are labeled and never reported as exact;
- p50 router overhead is below 25 ms on the documented dev reference machine.
- every tool resolves to `full` without an override; scope precedence and reset behavior are deterministic;
- formatter preserves all protected technical facts and never receives machine/internal artifacts.

### M2 — Context, repository, and built-in memory

Tasks:

1. Implement `ContextProvider`, candidate normalization, source refs, scoring, deduplication, packing, omissions, and bundle hashing.
2. Implement native repository scanning with ignore rules, bounded reads, text/path search, and content-hash cache.
3. Add optional symbol extraction only after text/path fallback passes.
4. Implement secret-redaction hooks and allowed-root enforcement before candidates enter persistence/cache.
5. Create SQLite migrations, connection pool, FTS search, memory CRUD, retention, and deletion.
6. Add context pipeline trace events without recording content.
7. Test symlinks, case sensitivity, Unicode, large files, binaries, generated files, cancellation, and concurrent repository changes.

Gate:

- golden ranking fixtures include all mandatory evidence within budget;
- identical input produces an identical ordered bundle and hash;
- no candidate outside allowed roots enters a bundle;
- SQLite survives interrupted writes and migrates from every prior fixture version;
- baseline demo builds context without optional tools.

### M3 — Execution, compressors, telemetry, daemon, and IPC

Tasks:

1. Implement argument-array execution, working-directory checks, environment allow/deny policy, timeout, cancellation, output caps, and exit propagation.
2. Implement streaming terminal compressor and raw-output handle retention.
3. Implement built-in extractive context compressor.
4. Implement request/provider/token event storage and stats queries.
5. Implement service composition, capability registry, graceful fallback, and health states.
6. Implement Unix socket and Windows named-pipe JSON-RPC transports.
7. Implement locking, discovery, readiness, graceful shutdown, stale-state recovery, and rotating logs.
8. Fuzz protocol framing, terminal parsers, and compressor inputs.

Gate:

- blocked commands never spawn;
- cancellation kills the child process tree on each supported OS;
- nonzero exit codes, error evidence, and truncation are preserved;
- daemon accepts concurrent bounded requests and shuts down cleanly;
- IPC cannot be used by another local user in platform tests;
- telemetry contains no fixture source strings or secrets.

### M4 — MCP, CLI, SDK, and npm packaging

Tasks:

1. Implement the MCP tools in the specified order, with annotations and schemas.
2. Implement CLI commands: `init`, `doctor`, `start`, `stop`, `status`, `stats`, `add`, `remove`, `context`, `exec`, `config`, `cache`, and `memory`.
3. Generate the TypeScript protocol client and build the ergonomic SDK wrapper.
4. Add daemon discovery, cancellation, deadlines, typed errors, and safe reconnect.
5. Build platform binaries in CI and package optional npm platform artifacts.
6. Implement the npm launcher, binary resolution, checksum verification for fallback download, signal forwarding, and clear unsupported-platform errors.
7. Run install/smoke/uninstall tests in clean, offline-capable environments.
8. Add CLI and MCP reference documentation generated from source metadata.
9. Implement read-only update discovery/resolution, signed manifest verification, and `token-shrinker update --check`.
10. Implement component ownership detection for read-only update reports; defer the managed-tool store until post-v1 activation work.
11. Implement `token-shrinker output get|set`, SDK output policy types, and `token_shrinker_format_final` mode/profile arguments.

Gate:

- an MCP inspector can list and invoke every advertised tool;
- CLI JSON outputs match committed schemas;
- Node SDK integration tests pass against stdio and daemon transports;
- npm tarballs contain only intended files and install on every release target;
- binary version, package version, protocol version, and schema version are reported together.
- update resolution rejects incompatible, unsigned, expired, or source-mismatched artifacts.
- CLI/MCP/SDK JSON remains byte-stable regardless of Caveman settings.

### M5 — Agent adapters and portable skill

Implement adapters one at a time: Claude Code, Codex CLI, Gemini CLI, OpenCode, then Aider.

For each adapter:

1. Document the supported public extension/configuration point and tested versions.
2. Detect whether the agent exists without mutating anything.
3. Produce a dry-run install plan.
4. Add only an owned, clearly labeled configuration fragment.
5. Validate by starting the MCP server and invoking `capabilities` plus `build_context` through the agent-compatible client path.
6. Support idempotent reinstall and scoped uninstall with rollback.
7. Add fixture tests for existing user configuration, malformed config, paths with spaces, and Windows/macOS/Linux conventions.
8. Write a minimal skill that teaches the agent when to route, build context, fetch omitted evidence, inspect warnings, and request execution approval.
9. Add a native-transport regression test: adapter installation and upgrade must not alter provider base URLs; Claude must remain MCP-only for Headroom/Token-Shrinker.

The skill must explicitly say that concise formatting applies only to final responses and that it must not discard warnings, citations, commands, or uncertainty.

Gate:

- every adapter passes detect/install/validate/reinstall/uninstall tests;
- unrelated config bytes remain unchanged where the target format permits;
- removing the adapter leaves the agent usable;
- missing Token-Shrinker produces an actionable agent-side message.
- Claude adapter fixtures preserve `/rc` eligibility and direct Anthropic transport.

### M6 — Optional providers

Implement in independent crates/modules:

1. Graphify context/graph provider.
2. Headroom context compressor.
3. RTK terminal compressor.
4. Claude-Mem provider.
5. Generic MCP memory provider.

For each provider:

- define minimum/maximum tested versions and capability probe;
- use a short startup and per-operation deadline;
- validate response schema and size;
- describe its data boundary in `doctor` and request warnings;
- implement circuit breaking after repeated failures;
- fall back to the built-in provider for that request;
- add contract tests using a fake provider and optional live tests excluded from default CI;
- benchmark incremental latency, reduction, and evidence recall.

Gate:

- force-unavailable, timeout, crash, malformed-response, and incompatible-version tests all fall back safely;
- `required = true` fails clearly instead of falling back;
- disabling all providers reproduces the passing M4 baseline demo;
- provider attribution is present in telemetry and response metadata.

### M7 — VS Code, security hardening, and public proof

Tasks:

1. Implement the thin VS Code extension and workspace-trust behavior.
2. Complete the threat model using assets, actors, trust boundaries, threats, mitigations, and residual risks.
3. Add dependency/license scanning, CodeQL, Rust advisory checks, secret scanning, fuzz corpus, and artifact SBOM generation.
4. Perform manual review of path canonicalization, command launch, IPC authorization, config editing, archive handling, and signed-manifest verification.
5. Create the deterministic baseline-versus-optimized demo command.
6. Run the full benchmark suite on documented reference hardware and publish raw JSON plus rendered Markdown.
7. Add a demo recording/GIF only after the script is reproducible in CI.
8. Ask an independent reviewer to run install, native-transport safety, threat-model, update-check, and demo checks from a clean machine.

Gate:

- no unresolved critical/high security findings;
- public demo meets quality and reduction thresholds;
- optional-provider-off demo passes;
- extension marketplace package passes static and clean-profile smoke tests;
- published benchmark claims link to raw reproducible data.
- signed update checks reject tampered, expired, downgraded, source-mismatched, and incompatible metadata;
- update checks never modify installed components or native provider endpoints.

### M8 — Release candidates to v1.0.0

Tasks:

1. Freeze protocol v1 and document compatibility rules.
2. Confirm Apache-2.0 headers/metadata and audit dependency-license compatibility.
3. Reserve/confirm npm scope, crate names, extension publisher, repository URL, security contact, and signing identities.
4. Generate changelog and migration guide from the last alpha.
5. Build signed binaries on protected CI runners; produce checksums, SBOM, and provenance attestations.
6. Publish a release candidate to non-default npm dist-tag and crate prerelease where supported.
7. Test new install, upgrade, downgrade/compatibility error, adapter add/remove, offline install, and uninstall on clean target machines.
8. Run the full benchmark and 24-hour daemon soak test.
9. Obtain maintainer approval of artifacts and results.
10. Tag `v1.0.0`, publish GitHub release, platform npm packages, umbrella CLI, SDK, adapters, crate(s), and VS Code extension in dependency order.
11. Verify public install commands and checksums, then promote npm dist-tag.
12. Monitor security contact, issue tracker, crash-free local smoke runs, and install failures; document rollback/yank procedure.
13. Publish the signed compatibility manifest only after all referenced artifacts are public and verified; test `update --check` from the previous release without modifying it.

Gate:

- all release acceptance criteria below pass;
- release can be installed without build tools on each supported npm platform;
- no package resolves a mismatched native binary;
- support and rollback owners are named.

## 14. Testing strategy

### Test layers

| Layer | Scope | Required examples |
|---|---|---|
| Unit | pure domain behavior | routing table, scoring, budget math, redaction, config merge |
| Output semantics | Caveman profiles | mode precedence, exact preservation, auto-clarity, multilingual boundaries |
| Property | invariants across generated inputs | determinism, token bounds, parser never panics, no path escape |
| Snapshot/golden | stable user/protocol output | JSON schemas, CLI JSON, citations, compressed logs |
| Contract | provider and adapter interfaces | healthy/degraded/timeout/malformed/incompatible providers |
| Integration | databases/processes/transports | migrations, cancellation, named pipe/socket, daemon lifecycle |
| End-to-end | agent-like workflows | MCP client -> context -> execution -> stats |
| Cross-platform | OS-specific behavior | paths, signals/job objects, ACL/permissions, npm binary selection |
| Update | resolver and transaction safety | mixed ownership, signatures, interruption, rollback, active-session deferral |
| Post-v1 scheduler/CI | opt-in discovery and delivery | no-change, changed release, quarantine, signed publication, default notification |
| Fuzz | untrusted parsers | JSON-RPC frames, log formats, config, provider responses |
| Security | trust boundaries | traversal, symlink escape, command injection, secret persistence |
| Performance | latency/resource regressions | router, context builder, compressor, cold start, concurrency |
| Soak | long-running daemon | cache invalidation, memory growth, log rotation, reconnect |

### Required test fixtures

- small polyglot repository with known symbols and duplicate files;
- ignored, generated, binary, large, Unicode, and symlinked files;
- terminal logs from Rust, npm, Python, and generic compilers/test runners;
- success, warning, failure, timeout, cancellation, and huge-output commands;
- SQLite databases at each historical migration version;
- every adapter's clean, existing, malformed, and partially configured states;
- fake Graphify/Headroom/RTK/memory servers with controllable failure modes;
- secret canaries that must never appear in logs, telemetry, or reports.
- historical installed-version sets, signed/expired/tampered manifests, partial downloads, locked binaries, failed migrations, and forced restart failures;
- agent environments with `/rc`-safe native transport plus intentionally unsafe base-URL overrides for `doctor` detection.
- output fixtures containing negation, exclusions, warnings, commands, paths, citations, code, units, numbers, and exact errors in every supported mode;
- fake upstream registries with unchanged, compatible, incompatible, yanked, replaced-digest, and malicious releases;
- post-v1 user-scope scheduler fixtures for Windows Task Scheduler, macOS launchd, and Linux systemd user timers, with a portable daemon-timer fallback.

### Quality assertions

Every optimization fixture declares:

- required evidence spans;
- useful but optional spans;
- forbidden/out-of-scope spans;
- maximum context budget;
- expected route and acceptable reason codes;
- expected answer/root cause where applicable.

Track required-evidence recall, citation correctness, forbidden-context leakage, token reduction, latency, and outcome equivalence.

## 15. Benchmark methodology

### Suites

1. `routing`: thousands of small deterministic requests.
2. `context`: cold/warm scans across repository sizes and languages.
3. `compression`: noisy tool outputs with known required lines.
4. `memory`: SQLite search/write/retention at increasing record counts.
5. `daemon`: cold start, request concurrency, reconnect, and steady-state memory.
6. `end-to-end`: real task bundles with baseline and optimized variants.
7. `providers`: built-in versus each optional integration and its failure fallback.

### Reproducibility fields

Every report records:

- Token-Shrinker commit/version and dirty state;
- fixture/repository commit;
- OS, architecture, CPU, RAM, filesystem, and power mode;
- Rust/Node/provider versions;
- config and active capabilities;
- tokenizer or estimator ID;
- warmup count, sample count, and raw samples;
- cache state and whether providers were already running.

### v1 performance budgets

Use these as initial gates, then revise only through an ADR backed by data:

- router: p50 < 25 ms, p95 < 75 ms;
- warm small-repo context build: p50 < 150 ms, p95 < 500 ms;
- daemon ready on reference machine: p50 < 500 ms;
- built-in terminal compression overhead: < 5% of command duration for commands over one second, with an absolute cap documented for short commands;
- daemon idle memory: < 100 MiB;
- public suite median comparable-token reduction: >= 30%;
- required-evidence recall: >= 95%;
- no statistically material task-success regression versus baseline.

Do not hide results that miss a target. Mark them as open release blockers or revise the target transparently before claiming success.

## 16. CI/CD and packaging

### Pull-request CI

Run jobs in parallel where possible:

- Rust format, clippy with warnings denied, tests, docs;
- TypeScript format, lint, typecheck, tests, build;
- schema generation/drift and protocol snapshots;
- dependency advisory, license, secret, and static analysis;
- Linux integration/fuzz smoke tests;
- Windows named-pipe/process-tree/path tests;
- macOS socket/process/path tests;
- reduced deterministic benchmark with regression thresholds;
- npm pack inspection and platform-selector tests.
- previous-stable compatibility checks, signed-manifest verification, and native-transport invariant tests.
- expired, tampered, downgraded, source-mismatched, and incompatible update-manifest fixtures.

Cache dependencies by lockfile/toolchain, not build outputs that can mask generation drift.

### Release matrix

| OS | Rust target | npm platform package |
|---|---|---|
| Windows x64 | `x86_64-pc-windows-msvc` | `win32-x64` |
| Windows arm64 | `aarch64-pc-windows-msvc` | `win32-arm64` |
| macOS x64 | `x86_64-apple-darwin` | `darwin-x64` |
| macOS arm64 | `aarch64-apple-darwin` | `darwin-arm64` |
| Linux x64 | `x86_64-unknown-linux-gnu` | `linux-x64-gnu` |
| Linux arm64 | `aarch64-unknown-linux-gnu` | `linux-arm64-gnu` |

Build on the native OS where signing or platform behavior requires it. Test the actual packaged archive, not the workspace binary.

### Versioning

- SemVer for CLI, SDK, adapter, and protocol-facing crates.
- One coordinated release version through `v1`; platform packages exactly match the umbrella CLI.
- Protocol major compatibility is explicit; additive optional fields require tolerant readers.
- CLI `--version --json` returns component, package, commit, protocol, schema, and build target versions.
- Pre-1.0 releases use `alpha`, `beta`, and `rc` npm dist-tags; never publish an unverified candidate as `latest`.

### Release artifact order

1. Create Git tag from a protected, green commit.
2. Build/test/sign native binaries and attach checksums/SBOM/provenance to a draft release.
3. Publish platform npm packages.
4. Publish umbrella CLI referencing exactly those versions.
5. Publish SDK and adapter packages.
6. Publish Rust crates in dependency order if public crate publishing is desired.
7. Publish VS Code extension.
8. Verify clean installs and public metadata.
9. Finalize GitHub release and promote npm dist-tag.

Use trusted publishing/OIDC where registries support it. Require protected environments and human approval for production publication. Never expose long-lived registry tokens to pull-request jobs.

## 17. Security and privacy workstream

Security is continuous, not an M7 cleanup. Track at least these threats:

| Threat | Required mitigation/test |
|---|---|
| malicious repository config | schema validation; cannot weaken user policy; untrusted-workspace gate |
| path traversal/symlink escape | canonical allowed-root checks before reads and writes |
| command/argument injection | argument-array APIs; explicit shell mode; approval and audit metadata |
| hostile terminal escape sequences | sanitize control sequences in rendered output while retaining raw handle safely |
| secret capture | streaming redaction before logs/cache/telemetry; canary tests |
| local IPC impersonation | per-user ACL/permissions, identity checks, bounded authentication token |
| compromised optional provider | minimal data, deadlines, size/schema validation, declared data boundary, fallback |
| malicious provider output | treat as untrusted data; no command interpretation; parser fuzzing |
| npm install compromise | platform packages, checksums, provenance, minimal scripts, offline path |
| update supply-chain attack | signed expiring manifest, pinned authoritative sources, anti-downgrade rules, staged verification |
| partial/self-update failure | journaled state machine, versioned activation, known-good manifests, full-set rollback |
| Remote Control regression | never proxy Claude transport; base-URL snapshot and adapter/update regression probes |
| archive traversal | hardened extraction or avoid runtime archive extraction |
| SQLite corruption/injection | parameters, transactions, backups/migration tests, integrity checks |
| denial of service | frame/file/output/time/memory limits, backpressure, bounded queues |
| cross-repository memory leakage | explicit scopes, default repo namespace, deletion and access tests |

Before public beta, document:

- all filesystem/data locations and retention defaults;
- all optional outbound network paths (none in baseline runtime);
- credential handling and redaction limitations;
- responsible disclosure process and response expectations;
- supported versions and patch policy;
- how users export, inspect, and delete local data.

## 18. Demo that proves token reduction

### Fixture design

Create `fixtures/demo-repo` with:

- a reproducible failing test caused by one source defect;
- multiple plausible but irrelevant modules;
- duplicate/generated documentation;
- a large build log with repeated progress and one key error chain;
- optional graph relationships that improve ranking but are not required;
- declared evidence: failing assertion, caller, root-cause definition, and relevant config;
- secret canaries that must be excluded/redacted.

Pin all files and expected hashes in `fixtures/demo-manifest.yaml`.

### Demo command behavior

`token-shrinker benchmark demo` must orchestrate, not fake, both variants:

1. Reset only the fixture's generated state.
2. Run the same documented task and command.
3. Baseline: include the declared raw context set and raw terminal output.
4. Optimized: route, retrieve, rank, pack, execute, and compress through normal public APIs.
5. Count both with the same tokenizer/estimator.
6. Validate evidence recall and expected root cause.
7. Write immutable JSON results and a Markdown comparison.
8. Exit nonzero when any acceptance threshold fails.

### Required demo report

```text
Task and fixture commit
Selected route and reason codes
Active providers and fallbacks
                       Baseline   Optimized   Change
Input/context tokens
Terminal-output tokens
Total comparable tokens
Required evidence recall
Citation correctness
Task/root-cause result
Routing latency
Context-build latency
Compression latency
Tokenizer and exact/estimated status
```

Run these scenarios in CI:

1. baseline versus built-ins only;
2. every optional provider forcibly absent;
3. optional provider timing out/malformed;
4. all compatible optional providers enabled in a separate, non-required live job.

## 19. Release acceptance criteria

### Functional

- `init`, `doctor`, `start`, `stats`, and `add` meet their CLI contracts.
- All nine MCP tools validate against published schemas.
- `FAST`, `BUILD`, and `DEEP` decisions are deterministic and explainable.
- Context bundles obey budgets and contain fetchable provenance.
- SQLite memory works with retention, scoping, inspection, and deletion.
- Execution preserves exit status and critical evidence under compression.
- Concise mode changes only final-response formatting.
- Caveman `full` is default globally and for every new agent/tool; all seven modes can be overridden by request, session, tool, or agent.
- Caveman formatting never changes machine payloads, raw evidence, reasoning, code, citations, warnings, or protected technical facts.
- `token-shrinker update --check` reports compatible releases and exact ownership-aware actions without changing the system.
- update checks reject expired, tampered, downgraded, source-mismatched, and incompatible metadata.
- Late optional features negotiate through capabilities and do not break clients within the documented compatibility window.

### Resilience

- Baseline workflow passes with no optional tools installed.
- Every optional adapter has absence, timeout, crash, malformed-output, and incompatible-version tests.
- Daemon reconnect, stale state, cancellation, process-tree cleanup, and graceful shutdown pass on every OS.
- Configuration edits are idempotent, scoped, validated, and recoverable.
- Update checks do not alter installed components, active sessions, data, or native provider endpoints.

### Quality and performance

- Public demo achieves >=30% median comparable-token reduction.
- Required-evidence recall is >=95% and citation correctness is 100% for required demo evidence.
- No material task-success regression is observed on the published suite.
- Latency and resource budgets in section 15 pass or have a reviewed public exception.
- Benchmark reports include raw data and complete reproducibility metadata.

### Security and privacy

- No critical/high findings remain open.
- Secret canaries never occur in telemetry, logs, caches, or published reports.
- IPC access-control tests pass.
- Execution and allowed-root policies pass adversarial tests.
- SBOM, checksums, provenance, vulnerability process, retention, and deletion documentation ship.

### Distribution and documentation

- Clean npm install works without a Rust toolchain on all advertised targets.
- Cargo/source install and npm offline/platform-package install are documented and tested.
- README, CLI/MCP/API reference, adapter guides, migration guide, security policy, contributing guide, code of conduct, and exact license ship.
- Public package names, URLs, maintainers, and security contact replace all placeholders.
- Upgrade, uninstall, rollback/yank, and compatibility behavior are tested.
- Signed compatibility metadata, ownership reporting, and package-manager update instructions are documented.

## 20. Codex execution protocol

When Codex builds this project, use this loop for every milestone:

1. Read this plan, current ADRs, relevant crate/package code, and the last milestone report.
2. Confirm the worktree state and preserve unrelated user changes.
3. Select the smallest vertical slice that ends in observable behavior.
4. Add or update a failing test/fixture first when practical.
5. Implement through the stable contracts; do not duplicate core behavior in adapters.
6. Run targeted tests, then the milestone's full checks.
7. Run the relevant benchmark and compare against the checked-in baseline.
8. Update schemas, generated types, docs, changelog, and ADRs when contracts change.
9. Record a short `docs/milestones/MN.md` report: delivered behavior, commands run, results, benchmark delta, known risks, and next slice.
10. Commit one coherent change with no generated/local secrets or unrelated edits.

Stop and request a maintainer decision when:

- an implementation would change a release invariant;
- a public protocol field or provider contract needs a breaking change;
- a license or third-party redistribution term is unclear;
- platform signing/publishing credentials or registry ownership is required;
- security policy would be weakened;
- a proposed feature or update would proxy native model transport, disrupt Remote Control, or require silent privilege escalation;
- a benchmark target can be met only by dropping required evidence.

## 21. Definition of done

Token-Shrinker is production-ready when a new user can install it from npm on every advertised platform, run `init` and `doctor`, connect an MCP-capable agent, complete the public demo with at least the declared reduction and evidence quality, inspect local statistics and provenance, check compatible updates without system mutation, remove all local memory/configuration, preserve native agent features such as Claude Remote Control, and continue using the baseline product when every optional integration is absent or broken.

The project is not done merely because it produces smaller text. It is done when the smaller context remains sufficient, measurable, reproducible, secure, portable, and honestly reported.
