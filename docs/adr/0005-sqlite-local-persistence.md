# ADR 0005: SQLite local persistence

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

The runtime needs portable local memory, content-free telemetry, migrations, retention, transactional deletion, and queries without requiring a separate database service.

## Decision

Use one SQLite database for durable memory, content-free telemetry, and ephemeral artifact metadata in separate tables. Store raw execution artifacts as separate expiring files addressed by opaque handles; do not place their bodies in SQLite. Start with forward migrations, integrity checks, explicit retention, repository/user scopes, and transactional metadata deletion. Raw content is excluded from telemetry. Detect FTS5 as a capability and provide a deterministic search fallback when unavailable.

## Consequences

### Benefits

- One portable file-backed dependency with transactional guarantees.
- Supports indexed metadata and FTS where available.
- Easy local inspection, backup, and deletion.

### Costs and risks

- Concurrent runtime modes need connection and migration coordination.
- FTS availability and Unicode behavior vary by build configuration.
- Database files can still leak sensitive memory if scoping/redaction fails.

## Alternatives considered

- Embedded key-value store: simpler records, weaker relational reporting and migration ergonomics.
- JSON files: transparent, poor concurrency and transactional deletion.
- External database: operationally incompatible with the baseline product.

## Follow-up decisions

- Specify artifact-directory permissions, encryption expectations, and secure deletion limits in the threat model.
- Select and benchmark the non-FTS fallback before the memory API is stabilized.

## Acceptance evidence

- Threat review of data classes and deletion semantics.
- Migration, interrupted-write, concurrency, and secret-canary tests.
