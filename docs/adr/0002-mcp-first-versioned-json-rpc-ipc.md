# ADR 0002: MCP-first integration and versioned JSON-RPC IPC

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Agents increasingly support MCP, while the CLI, SDK, and editor need a stable local transport that is easy to inspect and test. Transport-specific domain models would fragment the public contract.

## Decision

MCP is the primary agent-facing interface. Reusable local clients communicate with the daemon using framed JSON-RPC 2.0 over a per-user socket or named pipe. Both surfaces use versioned, language-neutral schemas derived from the Rust domain contract. Every request carries a request ID, protocol version, bounds, cancellation, deadlines, and structured errors.

## Consequences

### Benefits

- Standards-based integration for multiple agents.
- Human-inspectable local messages and generated clients.
- Domain behavior remains independent of transport.

### Costs and risks

- MCP and local IPC adapters both require compatibility testing.
- JSON framing and validation add overhead and untrusted-input surface.
- Schema evolution discipline is required early.

## Alternatives considered

- MCP for every client: possible, but less natural for reusable daemon discovery and SDK lifecycle.
- gRPC: strong tooling, heavier distribution and weaker direct MCP alignment.
- Ad hoc CLI subprocess calls: simple initially, poor streaming, cancellation, and typed-error behavior.

## Non-blocking implementation follow-ups

- Should local IPC expose the same method names and payloads as MCP or a smaller service API?
- What protocol compatibility window will pre-1.0 releases promise?

## Acceptance evidence

- Commit a protocol envelope schema and compatibility tests.
- Measure framing overhead against the router latency budget.
