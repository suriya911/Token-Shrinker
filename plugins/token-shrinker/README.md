# Token-Shrinker for Codex

This Codex plugin exposes the nine Token-Shrinker MCP tools and a workflow skill for bounded, evidence-preserving coding context.

## Install from the repository marketplace

```powershell
codex plugin marketplace add suriya911/Token-Shrinker
codex plugin add token-shrinker@token-shrinker-plugins
```

Start a new Codex task after installation so the MCP tools and skill are loaded.

## Verify

Ask Codex:

```text
Call token_shrinker_capabilities. Do not inspect files. Report the binary version,
package version, protocol version, health, and all five providers.
```

The first MCP launch uses `npx` to obtain the pinned public package `@token-shrinker/cli@0.1.1` from npm. npm caches the package for later launches. Token-Shrinker does not redirect model-provider endpoints; repository context, memory, and execution remain local.
