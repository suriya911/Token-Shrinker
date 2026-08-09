/** OpenCode MCP and Agent Skill configuration adapter. */
import type { AdapterDefinition } from "@token-shrinker/adapter-core";

export const openCodeAdapter: AdapterDefinition = {
  id: "opencode", displayName: "OpenCode", executable: "opencode",
  docsUrl: "https://opencode.ai/docs/mcp-servers/",
  testedVersions: "OpenCode 1.18.x MCP and Agent Skills contracts",
  integration: "mcp-stdio", skillRelativePath: ".opencode/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: "opencode.json", format: "json",
    jsonPath: ["mcp", "token-shrinker"],
    jsonValue: { type: "local", command: [binaryPath, "start", "--stdio"],
      enabled: true },
  }),
};

export default openCodeAdapter;
