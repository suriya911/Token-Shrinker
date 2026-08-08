# ADR 0003: Staged runtime modes

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Indexes, caches, memory, and telemetry benefit from a reusable process, but agents commonly own stdio MCP child lifecycles and some environments prohibit background services.

## Decision

Build the client-owned stdio MCP server and in-process one-shot CLI first. They share one Rust service graph, domain behavior, configuration, policy, schemas, and tests. Add the per-user daemon only after the core context workflow is proven and warm-process reuse has measured value. The daemon remains optional when introduced.

## Consequences

### Benefits

- Natural MCP lifecycle for clients that prefer stdio.
- A direct CLI path with fewer lifecycle and concurrency risks during alpha.
- A future daemon can reuse proven handlers instead of defining the architecture prematurely.

### Costs and risks

- One-shot and stdio startup cost may be higher until the daemon exists.
- SDK/editor connection reuse is deferred.
- A later daemon still requires platform lifecycle, authorization, and concurrency tests.

## Alternatives considered

- Daemon only: simpler runtime, fragile installation and restrictive environments.
- Stdio only: simple agent integration, poor reuse for SDK/editor clients.
- Separate implementations: rejected because behavior would drift.

## Follow-up decisions

- Define measurable latency or reuse evidence that justifies starting daemon work.
- Define migration ownership and database coordination before enabling concurrent daemon and stdio access.

## Acceptance evidence

- Lifecycle state-machine design for all three modes.
- Cross-platform prototype proving shared handlers in stdio and one-shot modes.
