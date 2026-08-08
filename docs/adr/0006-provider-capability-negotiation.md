# ADR 0006: Provider traits and capability negotiation

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Graph, compression, memory, tokenizer, and future providers offer useful enhancements but may be absent, incompatible, slow, malformed, or cross a different data boundary.

## Decision

Each domain owns its provider trait. Providers publish identity, version, capabilities, availability, limits, and data boundary. Optional providers are probed with bounded time and validated before registration. Failure selects a declared built-in local fallback and emits a structured warning. Startup fails only when configuration explicitly marks that provider as required. A network provider is never selected silently.

## Consequences

### Benefits

- Baseline behavior works without third-party tools.
- Degradation is visible and testable.
- New implementations can extend stable domain contracts.

### Costs and risks

- Provider contracts and fake-provider suites require maintenance.
- Feature intersection and version negotiation add complexity.
- Fallback output quality may differ from optional-provider output.

## Alternatives considered

- Generic plugin interface: fewer traits, weaker domain guarantees and oversized permissions.
- Compile-time optional providers only: simple binaries, poor runtime discovery and distribution.
- Hard dependencies: rejected because one failed tool would break the product.

## Non-blocking implementation follow-ups

- Which provider capabilities must be standardized in protocol v1?
- Is `required = true` allowed in repository config, or only user/CLI configuration?

## Acceptance evidence

- Fake-provider contract suite covering absence, timeout, crash, malformed output, and incompatible versions.
- Data-boundary information visible through `doctor` and response warnings.
