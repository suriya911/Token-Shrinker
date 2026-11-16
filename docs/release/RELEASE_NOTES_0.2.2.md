# Token-Shrinker 0.2.2

Token-Shrinker 0.2.2 is a pre-1.0 release focused on local agent setup, safe optional-provider
fallbacks, and release-candidate supply-chain evidence.

## Highlights

- Idempotent global setup and status for supported local integrations.
- Generated workflow guidance for Headroom cadence, Graphify project refresh, RTK terminal
  routing, Claude-Mem task/session search, and Caveman output defaults.
- Release artifacts now carry deterministic aggregate and per-file SHA-256 checksums.
- Release-candidate CI independently verifies a CycloneDX SBOM and emits SLSA build provenance.
- Ed25519 compatibility-manifest signing and independent verification tooling is ready for the
  protected post-publication ceremony.

## Compatibility

- Token-Shrinker protocol: `1.0`
- MCP protocol: `2025-11-25`
- Node.js: `>=24 <25`
- pnpm: `>=11 <12`
- Rust: `1.97.1`

## Important boundary

The local adapters can configure supported terminal and editor agents. They cannot globally
intercept browser-hosted ChatGPT, Gemini, or Claude web conversations without an official client
extension or connector. Optional providers fall back safely when unavailable.

## Verification

Run the release metadata, script, artifact, and compatibility-manifest checks described in the
[publishing runbook](./PUBLISHING.md). Public publication, signing-key use, independent review,
and clean-machine testing remain maintainer-controlled release gates.
