/** Cursor MCP and always-applied project rule adapter. */
import { automaticRule, type AdapterDefinition } from "@token-shrinker/adapter-core";

export const cursorAdapter: AdapterDefinition = {
  id: "cursor", displayName: "Cursor", executable: "cursor-agent",
  docsUrl: "https://docs.cursor.com/context/model-context-protocol",
  testedVersions: "current .cursor/mcp.json and .cursor/rules contracts",
  integration: "mcp-stdio", skillRelativePath: ".cursor/rules/token-shrinker.mdc",
  skillContent: automaticRule("cursor"),
  config: (binaryPath) => ({
    relativePath: ".cursor/mcp.json", format: "json",
    jsonPath: ["mcpServers", "token-shrinker"],
    jsonValue: { command: binaryPath, args: ["start", "--stdio"] },
  }),
};

export default cursorAdapter;
