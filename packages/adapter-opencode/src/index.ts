/** OpenCode MCP and Agent Skill configuration adapter. */
import type { AdapterDefinition } from "@token-shrinker/adapter-core";

export const openCodeAdapter: AdapterDefinition = {
  id: "opencode", displayName: "OpenCode", executable: "opencode",
  docsUrl: "https://opencode.ai/docs/mcp-servers/",
  testedVersions: "OpenCode MCP v2 and Agent Skills contracts",
  integration: "mcp-stdio", skillRelativePath: ".opencode/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: "opencode.json", format: "json",
    jsonPath: ["mcp", "servers", "token-shrinker"],
    jsonValue: { type: "local", command: [binaryPath, "start", "--stdio"] },
  }),
  user: {
    skillRelativePath: ".config/opencode/skills/token-shrinker/SKILL.md",
    config: (binaryPath) => ({
      relativePath: ".config/opencode/opencode.json", format: "json",
      jsonPath: ["mcp", "servers", "token-shrinker"],
      jsonValue: { type: "local", command: [binaryPath, "start", "--stdio"] },
    }),
  },
};

export default openCodeAdapter;
