# Token-Shrinker for Claude Code

This plugin installs the pinned public Token-Shrinker CLI package, starts its local MCP server,
and provides a skill for bounded, cited repository context. It does not redirect Anthropic model
traffic, credentials, or native model transport.

## Install from GitHub

```text
/plugin marketplace add suriya911/Token-Shrinker
/plugin install token-shrinker@token-shrinker-plugins
```

Restart Claude Code, approve the local `token-shrinker` MCP server when prompted, then call
`token_shrinker_capabilities` followed by `token_shrinker_build_context`.

## Verify

```text
/plugin list
/mcp
```

The plugin pins `@token-shrinker/cli@0.1.0`. Claude Code installs its dependencies with lifecycle
scripts disabled; Token-Shrinker's npm launcher selects the matching prebuilt native package.
