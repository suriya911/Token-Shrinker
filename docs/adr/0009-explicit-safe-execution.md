# ADR 0009: Explicit authorization and argument-array execution

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Running commands is useful for task evidence but crosses a major trust boundary. Shell interpolation, repository-controlled policy, path escape, inherited secrets, and incomplete process cleanup can create severe harm.

## Decision

Execution occurs only after an explicit CLI request or authenticated client authorization allowed by user-level policy. The engine launches executable and argument arrays directly. Shell mode is a separate explicit operation. It canonicalizes working directories, enforces allowed roots, controls environment inheritance, bounds time and output, supports cancellation, and preserves exit status plus critical evidence. Repository config cannot weaken user security policy.

## Consequences

### Benefits

- Smaller command-injection and surprise-execution surface.
- Consistent audit metadata and cross-platform policy.
- Blocked commands never spawn.

### Costs and risks

- Shell syntax and compound commands require an explicit less-safe path.
- Windows and Unix process-tree termination differ substantially.
- Strong defaults may require user overrides for legitimate workflows.

## Alternatives considered

- Always use a shell: convenient, unsafe by default.
- Never execute: safer, but prevents output optimization and end-to-end workflows.
- Trust the calling agent entirely: incompatible with defense in depth.

## Non-blocking implementation follow-ups

- Which environment variables are inherited by default?
- Is repository policy allowed to tighten execution only, or should it be ignored entirely until trust is established?

## Acceptance evidence

- Threat model and adversarial tests for injection, traversal, environment leakage, timeout, and cancellation.
- Platform tests proving child process-tree termination.
