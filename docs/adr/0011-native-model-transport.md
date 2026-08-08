# ADR 0011: Preserve native agent-to-model transport

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Redirecting an agent's model API can break authentication, vendor features, context behavior, Remote Control, and user trust. Token-Shrinker only needs to provide tools and optimized context.

## Decision

Token-Shrinker is a tool/runtime layer, not a universal model proxy. Default adapters must not set or rewrite provider base URLs, credentials, or model transport. Claude Code integrates through MCP so `/remote-control` remains on Claude's direct Anthropic connection. Any future transport wrapper is agent-specific, explicit, reversible, opt-in, and separately tested.

## Consequences

### Benefits

- Preserves vendor authentication and native features.
- Avoids handling model credentials and full prompt traffic.
- Keeps the trust boundary narrow and understandable.

### Costs and risks

- Some proxy-only optimization techniques are unavailable.
- Integration quality depends on each agent's public tool/context hooks.
- Compatibility tests must detect indirect transport changes from installers and updates.

## Alternatives considered

- Universal API proxy: broad visibility, unacceptable compatibility and credential risk.
- Agent patches: deep integration, brittle and outside public extension points.
- CLI-only context export: safe, but less ergonomic than MCP tools.

## Non-blocking implementation follow-ups

- Should transport wrapping be declared permanently out of scope rather than merely opt-in?
- Which environment/config keys must `doctor` inspect for every supported agent?

## Acceptance evidence

- Before/after adapter fixtures proving provider endpoints are byte-identical.
- Claude MCP integration test that retains Remote Control eligibility.
