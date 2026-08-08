/** OpenCode V2 MCP and Agent Skill configuration adapter. */
import type { AdapterDefinition } from "@token-shrinker/adapter-core";

export const openCodeAdapter: AdapterDefinition = {
  id: "opencode", displayName: "OpenCode", executable: "opencode2",
  docsUrl: "https://opencode.ai/v2/docs/mcp-servers",
  testedVersions: "current OpenCode V2 mcp.servers and Agent Skills contracts",
  integration: "mcp-stdio", skillRelativePath: ".opencode/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: "opencode.json", format: "json",
    jsonPath: ["mcp", "servers", "token-shrinker"],
    jsonValue: { type: "local", command: [binaryPath, "start", "--stdio"],
      disabled: false, codemode: false },
  }),
};

export default openCodeAdapter;
