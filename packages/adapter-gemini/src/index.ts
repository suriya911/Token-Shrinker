/** Gemini CLI MCP and skill configuration adapter. */
import type { AdapterDefinition } from "@token-shrinker/adapter-core";

export const geminiAdapter: AdapterDefinition = {
  id: "gemini", displayName: "Gemini CLI", executable: "gemini",
  docsUrl: "https://geminicli.com/docs/tools/mcp-server/",
  testedVersions: "current mcpServers and workspace Agent Skills contracts (2026-06-18 docs)",
  integration: "mcp-stdio", skillRelativePath: ".gemini/skills/token-shrinker/SKILL.md",
  config: (binaryPath) => ({
    relativePath: ".gemini/settings.json", format: "json",
    jsonPath: ["mcpServers", "token-shrinker"],
    jsonValue: { command: binaryPath, args: ["start", "--stdio"], timeout: 60_000, trust: false },
  }),
};

export default geminiAdapter;
