# MCP tool reference

Generated from the Rust MCP tool metadata. Do not edit manually.

## `token_shrinker_capabilities`

List versions, limits, providers, and degradation reasons.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_route`

Select and explain FAST, BUILD, or DEEP using deterministic rules.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_task_status`

Read the project-local task ledger and current active task.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_task_update`

Create or update the project-local task ledger without sending data outside the workspace.

- Read only: `false`
- Destructive: `false`
- Idempotent: `false`

## `token_shrinker_build_context`

Build a provenance-rich native repository context bundle.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_fetch_source`

Fetch a previously cited repository source by addressable handle.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_search_memory`

Search isolated local memory without external transport.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_remember`

Store an explicitly supplied local memory record.

- Read only: `false`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_execute`

Run an explicitly approved argument-array command under bounded policy.

- Read only: `false`
- Destructive: `true`
- Idempotent: `false`

## `token_shrinker_stats`

Return local content-free token savings aggregates.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`

## `token_shrinker_record_tokens`

Record an exact or estimated token measurement supplied by a compatible tokenizer.

- Read only: `false`
- Destructive: `false`
- Idempotent: `false`

## `token_shrinker_format_final`

Resolve the selected final-response profile without changing machine-readable payloads.

- Read only: `true`
- Destructive: `false`
- Idempotent: `true`
