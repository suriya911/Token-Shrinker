# ADR 0014: Freeze protocol v1 as an additive compatibility contract

- Status: Accepted
- Date: 2026-11-15
- Deciders: Project owner

## Context

Token-Shrinker is used by multiple MCP clients and agent adapters that may be
upgraded independently. A client must be able to continue using a v1 server
without learning every later extension, while incompatible changes must be
visible instead of being silently downgraded.

The implementation declares Protocol `1.0` and schema version `1` as the
current compatibility baseline. The wire fixture and protocol checks are the
repository evidence for that baseline.

## Decision

Freeze Protocol `1.0` and schema version `1` for the v1 release line.

- Minor extensions are additive: new optional fields, tools, capabilities,
  warnings, and enum values may be introduced without changing the major
  protocol version.
- Existing required fields, meanings, error codes, tool names, and transport
  framing cannot be removed, reinterpreted, or made stricter in v1.
- New required request fields, changed field types, incompatible framing, or
  incompatible semantics require Protocol v2 and a new ADR.
- Clients must ignore unknown response fields and servers must return a
  structured compatibility error for unsupported major versions. Mutating
  requests must never silently downgrade.
- Generated schemas, TypeScript types, JSON Schema, and the golden wire
  fixture must be updated together and pass the protocol gates.
- Agent adapters may evolve independently only when they preserve the wire
  contract and the agent's native model transport.

## Consequences

### Benefits

- Existing clients receive predictable compatibility behavior.
- Extensions can ship without forcing synchronized upgrades.
- Protocol drift is detected by generated artifacts and CI checks.

### Costs and risks

- Additive compatibility increases fixture and test maintenance.
- Breaking changes require a coordinated protocol-major release.
- Supporting older clients limits cleanup opportunities before v1 ends.

## Alternatives considered

- Lockstep client and server releases: simpler coordination, but unsuitable
  for independently installed agent adapters.
- Best-effort version negotiation: risks silent behavior changes for mutating
  requests.
- Unversioned permissive payloads: makes compatibility and failure handling
  ambiguous.

## Acceptance evidence

- `ProtocolVersion::CURRENT` is `1.0` and schema version is `1`.
- `docs/reference/protocol-v1-compatibility.md` defines the public rules.
- `fixtures/mcp/wire-v1.json` captures the frozen wire contract.
- `pnpm check:protocol` and `pnpm check:mcp-wire` reject incompatible drift.
- Adapter tests preserve native model transport and execution policy.
