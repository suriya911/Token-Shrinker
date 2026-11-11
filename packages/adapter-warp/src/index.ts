/** Warp local MCP and always-applied project rule adapter. */
import { automaticRule, type AdapterDefinition } from "@token-shrinker/adapter-core";

export const warpAdapter: AdapterDefinition = {
  id: "warp", displayName: "Warp", executable: "warp",
  docsUrl: "https://docs.warp.dev/guides/external-tools/using-mcp-servers-with-warp",
  testedVersions: "current .warp/.mcp.json and project rule contracts",
  integration: "mcp-stdio", skillRelativePath: ".warp/rules/token-shrinker.md",
  skillContent: automaticRule("warp"),
  config: (binaryPath) => ({
    relativePath: ".warp/.mcp.json", format: "json",
    jsonPath: ["mcpServers", "token-shrinker"],
    jsonValue: { command: binaryPath, args: ["start", "--stdio"] },
  }),
};

export default warpAdapter;
