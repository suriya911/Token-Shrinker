# Token-Shrinker 0.1.1

Token-Shrinker 0.1.1 is a patch release for explicit-source ranking and public AI-agent plugin distribution.

## Highlights

- Explicitly requested repository paths now outrank broad prose matches under bounded budgets.
- Nested hidden files such as `.mcp.json` remain discoverable and receive path-match evidence.
- Claude Code and Codex marketplace plugins provide the portable Token-Shrinker skill and local stdio MCP integration.
- Native model-provider endpoints and credentials remain unchanged.

## Compatibility

- Token-Shrinker protocol: `1.0`
- MCP protocol: `2025-11-25`
- Node.js: `>=24 <25`
- pnpm: `>=11 <12`
- Rust: `1.97.1`

## Verification

The release gate validates coordinated Cargo/npm/VS Code metadata, Rust formatting and Clippy, workspace tests, package contents, both plugin manifests, secret scanning, benchmark evidence, and native package smoke tests.
