# Protocol v1 compatibility

Protocol `1.0` and schema version `1` are frozen for the v1 release line.

## Compatibility rules

- Clients must send a supported major protocol version and must ignore unknown response fields.
- Servers may add optional fields, tools, capabilities, warnings, and enum values without changing the major version.
- Existing required fields, meanings, error codes, and tool names cannot be removed or reinterpreted in v1.
- New required request fields, changed field types, or incompatible transport framing require protocol v2.
- Generated Rust schemas, TypeScript types, JSON Schema, and reference fixtures must remain synchronized and pass `pnpm check:protocol`.
- An unsupported major version must produce a structured compatibility error; it must never silently downgrade a mutating request.
- Agent adapters may evolve independently when they preserve the protocol and do not redirect an agent's native model transport.

The CLI `version --json` response is the release compatibility probe. It reports binary, package, protocol, and schema versions so installers can reject mismatched native packages.
