# ADR 0004: Deterministic explainable routing

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Choosing `FAST`, `BUILD`, or `DEEP` affects latency, provider use, context budget, and execution eligibility. An LLM-based router would add cost, network dependence, nondeterminism, and another prompt-security boundary.

## Decision

Version 1 uses ordered deterministic rules over explicit mode, requested operations, named scope, repository breadth, source hints, and configured thresholds. Explicit valid overrides win. Ambiguity defaults to `BUILD`. Every decision returns stable reason codes and the effective budget. Version 1 does not expose a numeric confidence value because the heuristic is not statistically calibrated.

## Consequences

### Benefits

- Decisions are fast, testable, reproducible, and usable offline.
- Users can understand and override routing.
- No model credentials or inference cost are required.

### Costs and risks

- Heuristics may misclassify unusual requests.
- Goal parsing and thresholds need curated fixtures.

## Alternatives considered

- LLM classifier: potentially flexible, but violates baseline latency and offline goals.
- User-selected modes only: predictable, but pushes routine policy decisions onto every client.
- One fixed mode: operationally simple, wastes budget or omits evidence across task types.

## Follow-up decisions

- Curate the initial words, operations, and scope signals through public routing fixtures.
- Add confidence only through a later ADR backed by a calibration method and measured interpretation.

## Acceptance evidence

- Table-driven fixture suite covering priority and ambiguous cases.
- Determinism property test and routing microbenchmark.
