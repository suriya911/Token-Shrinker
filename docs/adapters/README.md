# Agent adapters

M5 adapters add Token-Shrinker through documented tool and instruction surfaces. They never set provider base URLs, credentials, models, or model-transport wrappers.

| Agent | Public integration | Owned project files | Tested contract |
|---|---|---|---|
| Claude Code | project MCP stdio + project skill | `.mcp.json` key and `.claude/skills/token-shrinker/SKILL.md` | Claude Code 2.1.224 and current [MCP](https://code.claude.com/docs/en/mcp)/[skills](https://code.claude.com/docs/en/skills) formats |
| Codex | project MCP stdio + portable skill | marked block in `.codex/config.toml` and `.agents/skills/token-shrinker/SKILL.md` | Codex CLI 0.145.0 and current [MCP configuration](https://learn.chatgpt.com/docs/extend/mcp) |
| Gemini CLI | project MCP stdio + workspace skill | `.gemini/settings.json` key and `.gemini/skills/token-shrinker/SKILL.md` | current [MCP](https://geminicli.com/docs/tools/mcp-server/) and [Agent Skills](https://geminicli.com/docs/cli/skills/) contracts |
| OpenCode | V2 local MCP stdio + Agent Skill | `opencode.json` key and `.opencode/skills/token-shrinker/SKILL.md` | current V2 [MCP](https://opencode.ai/v2/docs/mcp-servers) and [skills](https://opencode.ai/docs/skills) contracts |
| Aider | explicit owned config + read-only context | `.token-shrinker/aider.conf.yml`, context, and portable skill | current [YAML config](https://aider.chat/docs/config/aider_conf.html) `--config`/`read` contract |

## Lifecycle

Each TypeScript adapter exposes a definition consumed by `detectAdapter`, `planAdapter`, `applyAdapterPlan`, and `validateAdapter` from `@token-shrinker/adapter-core`.

1. Detection searches `PATH` and reads configuration without mutation.
2. Planning returns every before/after file state and performs no writes.
3. Installation writes only the named MCP key, a marked TOML fragment, or an owned sidecar.
4. Validation starts `token-shrinker start --stdio` and invokes both `token_shrinker_capabilities` and `token_shrinker_build_context`.
5. Reinstall is byte-idempotent. Uninstall removes only values that still match Token-Shrinker's owned value; modified values fail safely.
6. A failed multi-file apply restores earlier files to their prior bytes.

The Aider adapter does not claim MCP support. Launch Aider with `aider --config .token-shrinker/aider.conf.yml` after generating the task context. Missing Token-Shrinker must be reported as an installation problem; adapters must not silently substitute a model proxy.

Run `token-shrinker doctor --json` to see content-free warnings for provider endpoint overrides or wrapper recursion. Doctor reports names and remediation only; it never prints secret values or changes the environment.
