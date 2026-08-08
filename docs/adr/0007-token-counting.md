# ADR 0007: Exact tokenizers with labeled conservative estimates

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Token-Shrinker must enforce budgets and measure savings across models whose tokenizers may be known, unavailable, proprietary, or versioned independently. An unlabeled approximation would make claims misleading.

## Decision

Use an exact tokenizer only when its identity and compatibility are known. Otherwise use a documented conservative estimator and label every count as estimated with the estimator ID. Budget packing must not exceed the effective limit under the selected counter. Aggregates must not combine incompatible tokenizer identities without explicit normalization and labeling.

## Consequences

### Benefits

- Honest and reproducible accounting.
- Safe fallback without requiring a hosted tokenizer.
- Benchmark reports identify exactly how counts were produced.

### Costs and risks

- Estimates may underfill budgets.
- Maintaining exact tokenizer adapters adds size and compatibility work.
- Cross-model savings comparisons remain limited.

## Alternatives considered

- Character counts presented as tokens: simple but misleading.
- Require exact tokenizers: breaks universal baseline support.
- Ask provider APIs to count: adds network, credential, privacy, and latency dependencies.

## Non-blocking implementation follow-ups

- Which exact tokenizers are essential for the first alpha?
- What conservative estimator formula and safety margin should become part of the public contract?

## Acceptance evidence

- Corpus comparison of estimates against supported exact tokenizers.
- Property tests for nonnegative counts, determinism, and budget bounds.
