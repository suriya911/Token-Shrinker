/** Claude Code MCP and skill configuration adapter. */
import type { AdapterDefinition } from "@token-shrinker/adapter-core";

export const claudeCodeAdapter: AdapterDefinition = {
  id: "claude-code",
  displayName: "Claude Code",
  executable: "claude",
  docsUrl: "https://code.claude.com/docs/en/mcp",
  testedVersions: "2.1.224 and current project-scoped .mcp.json contract",
  integration: "mcp-stdio",
  skillRelativePath: ".claude/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: ".mcp.json", format: "json",
    jsonPath: ["mcpServers", "token-shrinker"],
    jsonValue: { type: "stdio", command: binaryPath, args: ["start", "--stdio"], env: {} },
  }),
};

export default claudeCodeAdapter;
