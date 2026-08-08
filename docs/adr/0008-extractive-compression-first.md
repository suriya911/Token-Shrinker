# ADR 0008: Extractive compression before abstractive compression

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Compression saves tokens only when required evidence remains accurate and traceable. Abstractive rewriting can introduce claims, change negation, or sever source ranges.

## Decision

The built-in baseline uses deterministic extractive techniques: deduplication, matched spans with surrounding context, structural headings, diagnostic retention, repeated-output collapsing, and bounded head/tail sections. Every omission remains fetchable through provenance. Abstractive providers, if later added, are optional and cannot replace the cited raw source.

## Consequences

### Benefits

- Strong provenance and semantic fidelity.
- Offline deterministic behavior.
- Straightforward golden and evidence-recall testing.

### Costs and risks

- Lower reduction on prose-heavy or diffuse material.
- Parsers and language-aware extraction still require maintenance.
- Extracted fragments can lose context unless span rules are careful.

## Alternatives considered

- LLM summarization by default: potentially smaller, but adds hallucination, privacy, latency, and cost risk.
- Truncation only: cheap, but commonly removes the decisive error or definition.
- No compression: preserves evidence but misses the product goal.

## Non-blocking implementation follow-ups

- Should any built-in operation paraphrase prose, or should all paraphrasing remain outside the evidence bundle?
- What minimum surrounding context is required for code, diagnostics, and prose?

## Acceptance evidence

- Golden fixtures with required spans, omissions, and fetch behavior.
- At least 95% required-evidence recall on the public suite before release claims.
