# ADR 0001: Rust core with a thin TypeScript edge

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Token-Shrinker needs predictable local latency, safe process and filesystem handling, cross-platform distribution, and integrations for npm-based agents and editors. Implementing optimization independently in Rust and TypeScript would create behavioral drift.

## Decision

Routing, ranking, token accounting, compression, execution policy, persistence, update policy, and telemetry live in Rust. TypeScript owns transport clients, generated protocol types, npm launchers, agent configuration adapters, and editor UX. TypeScript must call the Rust service rather than reimplement optimization decisions.

## Consequences

### Benefits

- One source of truth for security and optimization behavior.
- Native performance and bounded resource control on supported platforms.
- TypeScript remains convenient for agent and editor ecosystems.

### Costs and risks

- Cross-language schemas and release coordination are mandatory.
- Contributors need either Rust or TypeScript expertise, and some changes need both.
- Native packaging is more complex than a JavaScript-only tool.

## Alternatives considered

- TypeScript-only runtime: simpler distribution, weaker control over performance and native process behavior.
- Rust-only integrations: simpler language boundary, poorer npm/editor ergonomics.
- Two optimization engines: rejected because parity and security drift would be difficult to prove.

## Non-blocking implementation follow-ups

- Which operations, if any, may run in-process in the TypeScript SDK while preserving the boundary?
- Should generated TypeScript types be published from day one or only when the SDK begins?

## Acceptance evidence

- Agree on an ownership map for every planned crate and package.
- Prototype one Rust response consumed through generated TypeScript types without duplicated rules.
