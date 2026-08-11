# Agent adapters

M5 adapters add Token-Shrinker through documented tool and instruction surfaces. They never set provider base URLs, credentials, models, or model-transport wrappers.

| Agent | Public integration | Owned project files | Tested contract |
|---|---|---|---|
| Claude Code | project MCP stdio + project skill | `.mcp.json` key and `.claude/skills/token-shrinker/SKILL.md` | Claude Code 2.1.224 and current [MCP](https://code.claude.com/docs/en/mcp)/[skills](https://code.claude.com/docs/en/skills) formats |
| Codex | project MCP stdio + portable skill | marked block in `.codex/config.toml` and `.agents/skills/token-shrinker/SKILL.md` | Codex CLI 0.145.0 and current [MCP configuration](https://learn.chatgpt.com/docs/extend/mcp) |
| Gemini CLI | project MCP stdio + workspace skill | `.gemini/settings.json` key and `.gemini/skills/token-shrinker/SKILL.md` | current [MCP](https://geminicli.com/docs/tools/mcp-server/) and [Agent Skills](https://geminicli.com/docs/cli/skills/) contracts |
| OpenCode | local MCP stdio + Agent Skill | `opencode.json` key and `.opencode/skills/token-shrinker/SKILL.md` | OpenCode 1.18.x [MCP](https://opencode.ai/docs/mcp-servers/) and [skills](https://opencode.ai/docs/skills) contracts |
| Aider | explicit owned config + read-only context | `.token-shrinker/aider.conf.yml`, context, and portable skill | current [YAML config](https://aider.chat/docs/config/aider_conf.html) `--config`/`read` contract |

## Lifecycle

Each TypeScript adapter exposes a definition consumed by `detectAdapter`, `planAdapter`, `applyAdapterPlan`, and `validateAdapter` from `@token-shrinker/adapter-core`.

1. Detection searches `PATH` and reads configuration without mutation.
2. Planning returns every before/after file state and performs no writes.
3. Installation writes only the named MCP key, a marked TOML fragment, or an owned sidecar.
4. Server validation starts `token-shrinker start --stdio` directly and invokes both `token_shrinker_capabilities` and `token_shrinker_build_context`. This proves the generated command and Token-Shrinker protocol, not that the agent client approved or connected to the server.
5. Adapter results report `serverProtocolValidated`, `clientApproval`, `clientConnection`, and a human-readable detail separately. Client approval is never granted or bypassed by the installer. Claude project rejection is detected from `.claude/settings.local.json` and reported with the reset-and-approve remediation.
6. Reinstall is byte-idempotent. Uninstall removes only values that still match Token-Shrinker's owned value; modified values fail safely.
7. A failed multi-file apply restores earlier files to their prior bytes.

Large repository files are exposed to agents as deterministic `#Lx-Ly` ranges that fit bounded context requests. MCP responses return the first 100 ranked omissions plus `omissionSummary` totals so omission metadata cannot overwhelm the saved context. Generated skills require agents to fetch an omitted address before relying on it and forbid claims based only on filenames, source IDs, metadata, or prior knowledge.

The Aider adapter does not claim MCP support. Launch Aider with `aider --config .token-shrinker/aider.conf.yml` after generating the task context. Missing Token-Shrinker must be reported as an installation problem; adapters must not silently substitute a model proxy.

Run `token-shrinker doctor --json` to see content-free warnings for provider endpoint overrides or wrapper recursion. Doctor reports names and remediation only; it never prints secret values or changes the environment.

## Claude Code plugin marketplace

The repository also publishes a Claude Code plugin marketplace from
`.claude-plugin/marketplace.json`. The `token-shrinker` plugin bundles the portable skill and a
local MCP definition. Its locked npm dependency installs the coordinated Token-Shrinker CLI and
platform package with lifecycle scripts disabled.

```text
/plugin marketplace add suriya911/Token-Shrinker
/plugin install token-shrinker@token-shrinker-plugins
```

The plugin is additive to the transactional project adapter. Use the marketplace plugin for a
user-managed Claude installation; use `token-shrinker add claude-code` when a repository should
own and version its generated MCP and skill configuration.
