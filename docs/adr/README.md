# Architecture Decision Records

Architecture decision records (ADRs) capture durable choices that constrain Token-Shrinker's design. They explain context and tradeoffs; they are not substitutes for API documentation.

## Status lifecycle

`Proposed` -> `Accepted` -> `Superseded` or `Deprecated`

- Proposed ADRs are open for review and must not be treated as settled public contracts.
- Accepted ADRs require maintainer approval; unresolved items must be explicitly classified as non-blocking implementation follow-ups.
- A material reversal requires a new ADR that supersedes the old one.
- Implementation details may evolve without a new ADR when the decision and consequences remain intact.

## Index

| ADR | Decision | Status |
|---|---|---|
| [0001](./0001-rust-core-thin-typescript-edge.md) | Rust core with a thin TypeScript edge | Accepted |
| [0002](./0002-mcp-first-versioned-json-rpc-ipc.md) | MCP-first integration and versioned JSON-RPC IPC | Accepted |
| [0003](./0003-runtime-modes.md) | Staged runtime modes | Accepted |
| [0004](./0004-deterministic-router.md) | Deterministic explainable routing | Accepted |
| [0005](./0005-sqlite-local-persistence.md) | SQLite local persistence | Accepted |
| [0006](./0006-provider-capability-negotiation.md) | Provider traits and capability negotiation | Accepted |
| [0007](./0007-token-counting.md) | Exact tokenizers with labeled conservative estimates | Accepted |
| [0008](./0008-extractive-compression-first.md) | Extractive compression before abstractive compression | Accepted |
| [0009](./0009-explicit-safe-execution.md) | Explicit authorization and argument-array execution | Accepted |
| [0010](./0010-npm-native-packaging.md) | npm platform packages without an alpha downloader | Accepted |
| [0011](./0011-native-model-transport.md) | Preserve native agent-to-model transport | Accepted |
| [0012](./0012-transactional-updater.md) | Read-only v1 update checks; transactional updates later | Accepted |
| [0013](./0013-additive-extension-model.md) | Additive extensions with migration and rollback gates | Accepted |

Use [0000-template.md](./0000-template.md) for new decisions.
