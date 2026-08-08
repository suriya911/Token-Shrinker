# Token-Shrinker

> A local-first, MCP-first token-optimization runtime for AI and LLM command-line agents.

[![CI](https://img.shields.io/badge/CI-placeholder-lightgrey)](#continuous-integration)
[![npm](https://img.shields.io/badge/npm-placeholder-lightgrey)](#installation)
[![crates.io](https://img.shields.io/badge/crates.io-placeholder-lightgrey)](#installation)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](#license)

Token-Shrinker reduces the context and terminal output sent to an LLM while preserving the evidence needed to complete a task. It provides one local runtime for Claude Code, Codex CLI, Gemini CLI, OpenCode, Aider, editor extensions, and future MCP-capable agents.

The project uses a Rust core for low-latency routing, ranking, execution, storage, and IPC. TypeScript provides the npm installer, JavaScript SDK, agent adapters, and VS Code integration.

> **Project status:** design and implementation plan. APIs shown below are the target public contract until the first alpha release.

## Why Token-Shrinker?

AI coding agents commonly spend tokens on repeated files, irrelevant repository context, verbose command output, and stale session history. Token-Shrinker inserts a measurable optimization layer between an agent and those context sources.

It is designed to:

- reduce input and output tokens without hiding provenance;
- select a `FAST`, `BUILD`, or `DEEP` strategy for each request;
- rank and assemble context within an explicit token budget;
- compress terminal and log output while retaining errors and actionable evidence;
- reuse local memory without sending telemetry to a hosted service;
- work with no optional integrations installed;
- expose the same capabilities through MCP, a CLI, and an SDK;
- format final responses concisely when requested, without compressing internal reasoning.

## Architecture

```text
 Claude Code  Codex CLI  Gemini CLI  OpenCode/Aider  VS Code  Future Agents
      \           |          |            |            |          /
       +---------------- Agent adapters / MCP clients ----------------+
                                  |
                       stdio MCP or local IPC
                                  |
                  +---------------v----------------+
                  | token-shrinker daemon (Rust)   |
                  |                                |
                  | capability registry            |
                  | FAST / BUILD / DEEP router     |
                  | context builder + ranker       |
                  | execution engine               |
                  | token estimator + telemetry    |
                  | policy / redaction / cache     |
                  +------+---------+----------+----+
                         |         |          |
               +---------+--+  +---+----+  +--+----------------+
               | memory     |  | repo   |  | compression       |
               | SQLite     |  | native |  | built-in          |
               | Claude-Mem |  | graph  |  | Headroom / RTK    |
               | MCP source |  | Graphify|  | adapters          |
               +------------+  +--------+  +-------------------+
```

The daemon is the single source of truth for configuration, capabilities, budgets, caching, policy, and telemetry. Adapters do not implement optimization logic; they translate an agent's request into the stable protocol.

### Native agent transport and Remote Control safety

Token-Shrinker is a tool/runtime layer, not an LLM API proxy. It must not replace `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, model credentials, or an agent's native provider transport by default.

For Claude Code, Token-Shrinker and Headroom must integrate through MCP tools. Claude continues connecting directly to Anthropic, so native features such as `/remote-control` (`/rc`) remain available. A transport-wrapping integration may be offered for another agent only as an explicit, agent-specific option after a compatibility check; it must never become the universal default.

Updates and newly added features must preserve this invariant. The compatibility test suite launches each supported adapter, checks that its native provider endpoint is unchanged, and runs agent-specific feature probes where automation is possible. `token-shrinker doctor` reports any environment variable or wrapper that could redirect the native transport.

### Monorepo layout

```text
token-shrinker/
|-- Cargo.toml                    # Rust workspace
|-- package.json                 # pnpm workspace entry point
|-- pnpm-workspace.yaml
|-- rust-toolchain.toml
|-- crates/
|   |-- ts-types/                # shared IDs, errors, request/response models
|   |-- ts-config/               # layered config and capability discovery
|   |-- ts-router/               # deterministic FAST/BUILD/DEEP routing
|   |-- ts-context/              # candidates, ranking, budgets, provenance
|   |-- ts-memory/               # MemoryProvider + SQLite implementation
|   |-- ts-repo/                 # native repository scanner and Graphify adapter
|   |-- ts-compress/             # built-in, Headroom, and RTK adapters
|   |-- ts-output/               # Caveman profiles and user-facing format policy
|   |-- ts-exec/                 # policy-aware process execution and streaming
|   |-- ts-telemetry/            # local token/event accounting
|   |-- ts-update/               # signed compatibility manifest and read-only update checks
|   |-- ts-protocol/             # versioned JSON-RPC/MCP schemas
|   |-- ts-daemon/               # lifecycle, local IPC, service composition
|   |-- ts-mcp/                  # MCP server and tool handlers
|   `-- ts-cli/                  # token-shrinker binary
|-- packages/
|   |-- sdk/                     # @token-shrinker/sdk
|   |-- cli/                     # @token-shrinker/cli + native launcher
|   |-- adapter-core/            # shared adapter contracts
|   |-- adapter-claude-code/
|   |-- adapter-codex/
|   |-- adapter-gemini/
|   |-- adapter-opencode/
|   |-- adapter-aider/
|   `-- vscode/                  # Token-Shrinker extension
|-- skills/
|   `-- token-shrinker/SKILL.md  # portable agent instructions
|-- schemas/                     # generated JSON Schema, protocol snapshots
|-- fixtures/                    # deterministic benchmark/demo repositories
|-- benchmarks/                  # harness, baselines, reports
|-- docs/                        # design, threat model, adapter guides
|-- examples/
|-- scripts/                     # build, packaging, release verification
|-- .github/workflows/
|-- BUILD_PLAN.md
|-- CONTRIBUTING.md
|-- SECURITY.md
|-- CODE_OF_CONDUCT.md
`-- LICENSE
```

The `ts-` Rust crate prefix means **Token-Shrinker**, not TypeScript.

## How it works

1. An agent submits a goal, available context sources, and a token budget through MCP, the CLI, or the SDK.
2. The capability registry detects installed optional providers and records why any provider is unavailable.
3. The router selects a mode:
   - `FAST`: direct questions, small lookups, and low-risk commands;
   - `BUILD`: normal coding, debugging, and multi-file edits;
   - `DEEP`: broad architecture, unfamiliar repositories, or cross-cutting investigations.
4. Context providers produce candidates with source, location, content hash, freshness, estimated tokens, and relevance signals.
5. The ranker deduplicates candidates, applies policy/redaction, scores usefulness, and packs the best evidence into the budget.
6. If execution is requested and allowed, the engine runs the command and streams output through the built-in terminal compressor or an available RTK adapter. Headroom, when available, is used for context compression earlier in the pipeline.
7. The response contains optimized context, provenance, omissions, token estimates, and warnings. Raw source data remains addressable for follow-up retrieval.
8. Aggregate token events are written locally. Content is not stored in telemetry by default.
9. Caveman formatting shortens user-facing summaries and final answers. Default mode is `full`; every agent and tool can override it. It never alters hidden reasoning, raw evidence, machine-readable output, or tool safety checks.

## Caveman output profiles

Token-Shrinker uses Caveman as its default human-output formatter. Default mode is `full`.

| Mode | Behavior |
|---|---|
| `lite` | full sentences and articles; removes filler and hedging |
| `full` | default; fragments allowed, articles/filler dropped, technical detail preserved |
| `ultra` | shortest unambiguous form; each fact stated once |
| `wenyan-lite` | concise semi-classical Chinese |
| `wenyan-full` | strongly compressed classical Chinese |
| `wenyan-ultra` | maximum classical Chinese compression |
| `off` | normal uncompressed prose |

Configuration precedence is: request override, session override, tool override, agent override, global default. Every built-in or optional tool has an output profile, including router, context builder, memory, Graphify, Headroom, RTK, execution, telemetry/stats, doctor, updater, and adapter reports.

```toml
[output]
default = "full"
auto_clarity = true
preserve_code = true
preserve_citations = true
preserve_warnings = true

[output.agents]
claude-code = "full"
codex = "full"
gemini = "full"

[output.tools]
router = "ultra"
context = "full"
memory = "full"
graphify = "full"
headroom = "full"
rtk = "ultra"
execution = "full"
stats = "ultra"
doctor = "lite"
update = "lite"
```

`auto_clarity = true` temporarily uses clearer prose for security warnings, irreversible actions, ordered recovery steps, or any case where compression could change meaning. It resumes configured mode afterward. Code, commands, paths, error strings, numbers, units, citations, uncertainty, and words such as `not`, `never`, and `only` are never dropped.

Caveman applies only after a tool result is normalized. Raw tool/MCP payloads, JSON/CSV, logs retained for provenance, source code, internal context, prompts, reasoning, and third-party messages remain unchanged. Agents can fetch the raw result when a compressed summary lacks needed evidence.

```bash
token-shrinker output get
token-shrinker output set full
token-shrinker output set --tool rtk ultra
token-shrinker output set --agent claude-code lite
token-shrinker output set --session off
```

### Routing defaults

| Mode | Typical trigger | Context budget | Providers | Execution |
|---|---|---:|---|---|
| `FAST` | focused lookup or known file | 4K tokens | native scan + recent memory | off unless requested |
| `BUILD` | implementation or debugging | 16K tokens | native repo, memory, optional graph/compression | allowed by policy |
| `DEEP` | architecture or repository-wide analysis | 48K tokens | all available providers, iterative retrieval | allowed by policy |

Budgets are configuration defaults, not hard-coded model limits. Explicit user settings override automatic routing. The router is deterministic and explainable in v1; an LLM is not required to decide the mode.

## Graceful degradation

The baseline product requires only the Token-Shrinker binary.

| Missing component | Fallback behavior |
|---|---|
| Graphify | native file metadata, text search, and optional tree-sitter symbols |
| Headroom | built-in extractive context compressor |
| RTK | built-in terminal compressor with error/summary/tail retention |
| Claude-Mem | built-in SQLite memory |
| external MCP memory | built-in SQLite memory |
| daemon | CLI may start it automatically or use one-shot in-process mode |
| exact model tokenizer | conservative byte/character estimator with an `estimated` label |

`token-shrinker doctor --json` exposes degraded capabilities in a machine-readable form. Optional-provider errors become warnings unless a provider was explicitly marked `required`.

## Extensibility and safe upgrades

Late features are added behind versioned provider, adapter, MCP, and protocol interfaces. New fields are additive within a protocol major version, unknown optional capabilities are ignored safely, and database changes use forward-only migrations with a tested backup/restore path. A feature cannot bypass execution policy, replace native model transport, or make an optional tool mandatory.

Core v1 provides a read-only compatibility check:

```bash
token-shrinker update --check
```

The command verifies a signed, expiring manifest, reports the latest compatible stable versions, identifies component ownership, and prints the exact package-manager actions required. It never downloads, installs, activates, or replaces a component.

Managed transactional updates, rollback, background schedulers, and unattended activation are post-v1 work. They require a separate readiness review and supply-chain threat model. If update automation is later introduced, its initial default will be `notify`, not unattended application.

## Installation

### npm (recommended for agents and editors)

```bash
npm install --global @token-shrinker/cli
token-shrinker init
token-shrinker doctor
```

The npm launcher selects an exact-version optional platform package and never downloads or compiles an executable during installation or runtime. The continuously tested release targets are:

- Windows x64;
- macOS x64 and arm64;
- Linux x64 using glibc.

### Cargo

```bash
cargo install token-shrinker-cli
token-shrinker init
```

### From source

```bash
git clone https://github.com/suriya911/Token-Shrinker.git
cd token-shrinker
corepack enable
pnpm install --frozen-lockfile
cargo build --workspace
pnpm build
```

## Quick start

```bash
# Create local config and register the current repository.
token-shrinker init

# Explain installed, missing, and degraded capabilities.
token-shrinker doctor

# Start the local daemon. Use --stdio for a dedicated MCP process.
token-shrinker start

# Add an adapter or optional provider.
token-shrinker add codex
token-shrinker add graphify

# Show local savings by session, agent, mode, or date.
token-shrinker stats --since 7d
```

### Optimize context from the CLI

```bash
token-shrinker context build \
  --goal "Find the authentication regression and propose a fix" \
  --mode build \
  --budget 12000 \
  --format json
```

### Execute with compressed output

```bash
token-shrinker exec -- cargo test --workspace
```

By default, execution requires an explicit CLI request or MCP client consent and obeys repository policy. Token-Shrinker does not silently run commands merely to build context.

### JavaScript/TypeScript SDK

```ts
import { TokenShrinkerClient } from "@token-shrinker/sdk";

const client = await TokenShrinkerClient.connect();

const result = await client.buildContext({
  goal: "Explain why checkout tests fail",
  mode: "build",
  budget: { maxInputTokens: 12_000 },
  sources: [{ kind: "repository", root: process.cwd() }],
});

console.log(result.context);
console.log(result.metrics.estimatedTokensSaved);
```

## CLI reference

| Command | Purpose |
|---|---|
| `token-shrinker init` | create user/repository config and initialize local storage |
| `token-shrinker doctor [--json]` | diagnose binary, daemon, adapters, permissions, and fallbacks |
| `token-shrinker start [--stdio|--foreground]` | run the daemon or a dedicated MCP stdio server |
| `token-shrinker stop` | stop the current user's daemon |
| `token-shrinker status` | report daemon and protocol status |
| `token-shrinker stats [--since DURATION]` | display local token and latency metrics |
| `token-shrinker output get|set` | inspect or override Caveman mode globally, per agent/tool, or per session |
| `token-shrinker update --check` | verify signed compatibility metadata and report available updates without changing the system |
| `token-shrinker add <integration>` | install or configure an agent/provider integration |
| `token-shrinker remove <integration>` | remove generated integration configuration |
| `token-shrinker context build ...` | build a provenance-rich context bundle |
| `token-shrinker exec -- <command>` | execute under policy and compress the output |
| `token-shrinker config get|set|validate` | inspect or update layered configuration |
| `token-shrinker cache prune` | remove expired, non-memory cache entries |
| `token-shrinker memory list|forget` | inspect metadata or delete local memories |

All data-producing commands support `--json`. Stable automation should consume JSON rather than terminal prose.

## MCP tools

The MCP server advertises tools according to detected capabilities:

| Tool | Mutability | Description |
|---|---|---|
| `token_shrinker_capabilities` | read-only | list versions, limits, providers, and degradation reasons |
| `token_shrinker_route` | read-only | select/explain `FAST`, `BUILD`, or `DEEP` |
| `token_shrinker_build_context` | read-only | rank and pack context with provenance and omissions |
| `token_shrinker_fetch_source` | read-only | retrieve a cited source or omitted range on demand |
| `token_shrinker_search_memory` | read-only | query the configured memory provider |
| `token_shrinker_remember` | writes local/provider memory | store an explicitly supplied memory record |
| `token_shrinker_execute` | executes a process | run an approved command and compress its output |
| `token_shrinker_stats` | read-only | return local token and latency aggregates |
| `token_shrinker_format_final` | read-only | apply selected Caveman profile to final user-facing text only |

Every result includes `protocolVersion`, `requestId`, and structured warnings. Context results also include source citations, hashes, estimated token counts, budget decisions, and whether an exact tokenizer was used.

## Supported adapters

| Adapter | Target integration | Planned status |
|---|---|---|
| MCP stdio | any MCP-capable agent | first stable interface |
| Claude Code | generated MCP/skill configuration | milestone 5 |
| Codex CLI | generated MCP/skill configuration | milestone 5 |
| Gemini CLI | generated MCP configuration/instructions | milestone 5 |
| OpenCode | MCP/plugin configuration | milestone 5 |
| Aider | CLI wrapper/context export | experimental milestone 5 |
| VS Code | commands, status, diagnostics, stats | milestone 6 |
| JavaScript SDK | Node.js clients and future adapters | milestone 4 |

Adapter support means the integration is tested against documented public extension points. Token-Shrinker does not patch or impersonate an agent.

## Configuration

Configuration is layered from defaults, user config, repository config, environment variables, and CLI flags. Higher layers override lower ones. Repository configuration cannot weaken user-level security policy unless the user explicitly permits it.

```toml
version = 1

[router]
default_mode = "auto"
fast_budget = 4000
build_budget = 16000
deep_budget = 48000

[memory]
provider = "sqlite"
retention_days = 90

[providers.graphify]
enabled = "auto"

[providers.headroom]
enabled = "auto"

[providers.rtk]
enabled = "auto"

[execution]
enabled = true
workspace_only = true
approval = "client"

[telemetry]
enabled = true
content = false

[updates]
channel = "stable"

[final_output]
mode = "full" # lite | full | ultra | wenyan-lite | wenyan-full | wenyan-ultra | off
auto_clarity = true
```

Secrets do not belong in repository config. Provider credentials are referenced through environment variables or the operating system credential store.

## Development

Prerequisites:

- stable Rust from `rust-toolchain.toml`;
- Node.js active LTS;
- pnpm through Corepack;
- Git;
- optional tools only when developing their adapters.

```bash
pnpm install --frozen-lockfile
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm lint
pnpm test
pnpm build
```

Run an end-to-end development server:

```bash
cargo run -p ts-cli -- start --foreground
pnpm --filter @token-shrinker/sdk test:e2e
```

Protocol types originate from versioned Rust schema definitions and generated JSON Schema. Generated TypeScript types are checked into releases and verified for drift in CI.

## Benchmarking

Token-Shrinker reports both reduction and quality. A smaller prompt that removes required evidence is a failure, not an optimization.

```bash
token-shrinker benchmark run --suite fixtures/benchmark-suite.yaml
token-shrinker benchmark compare benchmarks/baseline.json benchmarks/current.json
```

Each fixture runs a baseline and optimized path against the same pinned repository snapshot and task. Reports include:

- raw versus optimized input tokens;
- raw versus compressed tool-output tokens;
- p50/p95 routing and context-build latency;
- peak memory and daemon cold-start time;
- citation coverage and required-evidence recall;
- task outcome or golden-answer checks;
- provider and fallback path used;
- exact tokenizer or estimator identity.

The first stable release target is at least **30% median token reduction** on the public fixture suite, **95% required-evidence recall**, no material task-success regression, **under 25 ms p50** routing overhead, and **under 150 ms p50** context-build overhead on the documented reference machine, excluding provider startup and model latency.

Benchmark data, hardware, OS, configuration, repository commit, and commands must ship with each published claim.

## Demo walkthrough

The repository will include a deterministic demo fixture with noisy build logs, duplicated documentation, a small source graph, and one relevant failing test.

```bash
# 1. Verify the baseline-only installation.
token-shrinker doctor

# 2. Capture the unoptimized task bundle and output.
token-shrinker benchmark demo --variant baseline --output demo-baseline.json

# 3. Run the same task through automatic routing and context selection.
token-shrinker benchmark demo --variant optimized --output demo-optimized.json

# 4. Produce a human-readable, reproducible comparison.
token-shrinker benchmark compare demo-baseline.json demo-optimized.json
```

The demo passes only if:

1. both variants identify the same root-cause file and failing assertion;
2. the optimized bundle includes citations for every required fact;
3. optimized input plus tool-output tokens are at least 30% lower;
4. the report names the tokenizer/estimator and all active fallbacks;
5. rerunning from the pinned fixture yields equivalent results within the declared tolerance;
6. disabling every optional adapter still produces a passing optimized run.

An optional second pass enables Graphify, Headroom, RTK, and an external memory provider to show incremental savings without making them prerequisites.

## Security and privacy

Token-Shrinker is local-first. The default build has no hosted telemetry endpoint and does not send prompts, source code, command output, or metrics to the project maintainers.

- IPC is limited to the current user and authenticated with an ephemeral local token where the platform requires it.
- Unix sockets use restrictive permissions; Windows uses a per-user named pipe with an access control list.
- The execution engine uses argument arrays, not shell interpolation, unless the caller explicitly chooses shell mode.
- Repository policy can deny execution, restrict working directories, cap output/time, and require client approval.
- Updates never rewrite model-provider base URLs, silently install shell wrappers, elevate privileges, or restart an active agent session.
- Update checks accept only signed, non-expired compatibility manifests from the declared release source and never activate artifacts in v1.
- Context sources are canonicalized and checked against allowed roots to prevent traversal and symlink escapes.
- Secret-like values are redacted before persistence and telemetry; users can add patterns.
- Telemetry stores counts, timings, provider IDs, and hashes by default, never source or prompt bodies.
- Memory writes are explicit, inspectable, scoped, and deletable.
- Optional providers receive only the minimum data required and are labeled at the protocol boundary.
- Release artifacts include checksums, provenance/attestations, and a software bill of materials.
- Dependencies and published packages are scanned in CI.

See `SECURITY.md` for vulnerability reporting and `docs/threat-model.md` for trust boundaries before the first public release.

## Continuous integration

Pull requests run formatting, linting, unit tests, protocol compatibility tests, integration tests, security checks, license checks, and a reduced benchmark suite on Windows, macOS, and Linux. Release candidates additionally build every target, install each npm artifact in a clean environment, verify checksums/signatures, run smoke tests, and execute the full benchmark suite.

## Recommended complementary tools

Keep these optional and capability-detected:

- [ast-grep](https://astgrep.com/) — best next context tool. Fast Rust-based structural search and rewriting. Use for targeted AST matches and codemod previews when Graphify is absent or a full graph is unnecessary.
- [The Update Framework](https://theupdateframework.io/docs/overview/) — strongest addition for unattended updates. Use its signed root/targets/snapshot/timestamp model to resist rollback, freeze, and compromised-key attacks.
- [Sigstore Cosign](https://docs.sigstore.dev/cosign/signing/overview/) — sign binaries, manifests, SBOMs, and provenance through CI identity; verify before automatic activation.
- [MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector) — test tools, resources, schemas, errors, and transports across every MCP-compatible agent.
- [Criterion.rs](https://bheisler.github.io/criterion.rs/book/) — regression-aware Rust microbenchmarks for router, ranker, compressor, IPC, and updater hot paths.

Recommended order for the core product: MCP Inspector and Criterion.rs first, then ast-grep after native search. Evaluate TUF plus Cosign before any post-v1 managed activation work because self-update expands supply-chain risk.

## Roadmap

- [x] M0: repository, governance, schemas, fixtures, and CI skeleton
- [x] M1: Rust core contracts, deterministic router, token estimation
- [x] M1.1: Caveman profile engine with `full` default and scoped overrides
- [x] M2: context builder/ranker, native repository provider, SQLite memory
- [x] M3: execution engine, built-in compressors, telemetry, daemon/IPC
- [x] M4: MCP server, CLI, TypeScript SDK, npm native packaging, update manifest
- [ ] M5: Claude Code, Codex CLI, Gemini CLI, OpenCode, and Aider adapters
- [ ] M6: Graphify, Headroom, RTK, Claude-Mem/external MCP memory adapters
- [ ] M7: VS Code extension, public demo, benchmark report, and security review
- [ ] M8: release candidates, compatibility freeze, and `v1.0.0`
- [ ] Post-v1: transactional managed updater, rollback, compatibility watcher, and opt-in scheduler

Detailed tasks, dependencies, gates, and acceptance criteria are in [BUILD_PLAN.md](./BUILD_PLAN.md).

## Contributing

Contributions are welcome after the repository governance documents are published.

1. Read `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and the architecture decision records.
2. Open an issue before changing the public protocol or provider contracts.
3. Add tests and benchmark evidence for behavior or performance changes.
4. Keep optional integrations behind capability detection and a tested fallback.
5. Run the full local validation commands before opening a pull request.

Good first contributions include new deterministic fixtures, token-estimator tests, documentation, and adapters built against public extension points.

## License

Token-Shrinker is licensed under the [Apache License 2.0](./LICENSE).

Third-party optional tools retain their own licenses and are not bundled unless their licenses and distribution terms explicitly allow it.

## Project placeholders

- Repository: `https://github.com/OWNER/token-shrinker`
- npm scope: `@token-shrinker/*` (confirm availability)
- Rust crate names: confirm availability before publishing
- Maintainers: `MAINTAINERS_TBD`
- Security contact: `SECURITY_CONTACT_TBD`
- License: `Apache-2.0`
