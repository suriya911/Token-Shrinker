/** Codex MCP and portable skill configuration adapter. */
import { tomlString, type AdapterDefinition } from "@token-shrinker/adapter-core";

export const codexAdapter: AdapterDefinition = {
  id: "codex", displayName: "Codex", executable: "codex",
  docsUrl: "https://learn.chatgpt.com/docs/extend/mcp",
  testedVersions: "codex-cli 0.145.0 and current project .codex/config.toml contract",
  integration: "mcp-stdio", skillRelativePath: ".agents/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: ".codex/config.toml", format: "toml-fragment",
    content: `[mcp_servers.token-shrinker]\ncommand = ${tomlString(binaryPath)}\nargs = ["start", "--stdio"]\nstartup_timeout_sec = 10\ntool_timeout_sec = 60`,
  }),
};

export default codexAdapter;
