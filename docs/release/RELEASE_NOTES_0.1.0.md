# Token-Shrinker 0.1.0

Token-Shrinker 0.1.0 is the first public release candidate. It provides a local-first,
evidence-preserving context pipeline for coding agents without redirecting native model
transport or credentials.

## Highlights

- Native Rust CLI and MCP server with bounded context building, source fetching, routing,
  local memory, approved execution, output policy, and content-free savings statistics.
- Downloader-free npm launcher with native packages for Windows x64, Linux x64 glibc,
  macOS x64, and macOS arm64.
- Transactional adapters for Claude Code, Codex, Gemini CLI, OpenCode, and Aider.
- VS Code extension for health, bounded context, and local statistics.
- Deterministic public benchmark demonstrating at least 50% token reduction while retaining
  required evidence and citations.
- Root-confined repository reads, secret redaction, bounded process execution, authenticated
  local transport, checksums, SBOMs, and artifact attestations.

## Verified local surfaces

- Native Windows x64 binary and npm package installation.
- Claude Code and Codex live MCP tool calls.
- VS Code VSIX installation, status, context building, and statistics.
- Synthetic integration contracts for all five agent adapters.

## Publication status

This commit prepares version-coordinated release artifacts. Public npm and VS Code
Marketplace publication remains disabled until the release owner verifies registry names,
configures protected publishing identities, and approves the release workflow.

## Compatibility

- Token-Shrinker protocol: `1.0`
- MCP protocols: `2025-11-25` and compatibility negotiation for `2025-06-18`
- Node.js: `>=24 <25`
- pnpm: `>=11 <12`
- VS Code: `>=1.125.0`
