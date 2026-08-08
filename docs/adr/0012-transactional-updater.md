# ADR 0012: Read-only v1 update checks; transactional updates later

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

The planned product coordinates a runtime, adapters, schemas, optional providers, and editor packages. Updating them independently can produce incompatible or partially activated installations. Automatic updates also create a high-value supply-chain boundary.

## Decision

Core v1 implements a read-only `update --check` using a signed, expiring compatibility manifest that pins authoritative source, version, platform, artifact identity, digest, protocol ranges, and known conflicts. It reports compatible updates and exact ownership-aware actions but does not activate them.

Transactional managed activation, rollback, schedulers, and unattended application are post-v1 work requiring a separate readiness review. When automatic updating is introduced, its initial default is `notify`, not unattended `apply`. External package-manager installations remain externally owned unless explicitly imported.

## Consequences

### Benefits

- V1 gains compatible, provenance-aware update information without a self-modifying runtime.
- The optimizer can ship and mature before taking on updater-level supply-chain risk.
- The later transaction design retains a clear security direction.

### Costs and risks

- V1 users apply updates through the owning package manager rather than one automatic transaction.
- Coordinated activation and rollback are deferred.
- Even read-only manifests require protected signing, expiry, and source-identity verification.

## Alternatives considered

- Delegate every update to existing package managers: simpler, cannot coordinate compatibility or rollback across managers.
- Notify only: safer and smaller, but does not meet the planned one-command managed experience.
- In-place self-update: simpler, unreliable on Windows and difficult to roll back safely.

## Follow-up decisions

- Select a signing framework and key-rotation model that the named maintainer team can operate safely.
- Require a new readiness decision before managed activation ships.
- Retain `notify` as the initial automation default unless a later ADR documents evidence for changing it.

## Acceptance evidence

- Separate threat model and maintainer ownership for signing/recovery.
- V1 manifest verification tests for expiry, tampering, downgrade, identity mismatch, and compatibility conflicts.
- Before post-v1 activation: failure injection at every state with complete rollback on Windows, macOS, and Linux.
