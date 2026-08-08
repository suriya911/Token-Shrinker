# ADR 0013: Additive extensions with migration and rollback gates

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Token-Shrinker will gain providers, agents, output modes, schema fields, and storage changes after initial clients exist. Late features must not silently break older adapters or weaken release invariants.

## Decision

Add capability flags before depending on new behavior. Protocol-minor fields are optional with safe defaults and tolerant readers. Experimental features are opt-in and separately labeled. Database changes use forward migrations plus tested backup/restore or explicit rollback support. Optional providers remain optional in minor releases. Breaking public contracts require a new ADR and protocol or package major version.

## Consequences

### Benefits

- Older clients continue operating within a documented window.
- New features can ship independently behind negotiated capabilities.
- Migration and rollback behavior becomes part of feature completeness.

### Costs and risks

- Tolerant readers and compatibility fixtures add ongoing test burden.
- Supporting old versions slows cleanup and schema simplification.
- Capability combinations can become difficult to reason about.

## Alternatives considered

- Lockstep upgrades only: simpler contracts, poor ecosystem resilience.
- Unversioned permissive payloads: flexible initially, ambiguous compatibility.
- Permanent backwards compatibility: unrealistic and costly before 1.0.

## Non-blocking implementation follow-ups

- What compatibility window applies before and after 1.0?
- Who can approve removal of an old capability or migration fixture?

## Acceptance evidence

- Written SemVer/protocol compatibility policy.
- Fixtures for older readers, unknown optional fields, capability absence, and every supported migration origin.
