/** Aider owned-config and read-only context adapter. */
import { aiderContext, type AdapterDefinition } from "@token-shrinker/adapter-core";

export const aiderAdapter: AdapterDefinition = {
  id: "aider", displayName: "Aider", executable: "aider",
  docsUrl: "https://aider.chat/docs/config/aider_conf.html",
  testedVersions: "current --config and read-only file contract",
  integration: "aider-context", skillRelativePath: ".token-shrinker/SKILL.md",
  config: () => ({
    relativePath: ".token-shrinker/aider.conf.yml", format: "owned-text",
    content: "# token-shrinker-owned:v1\nread:\n  - .token-shrinker/aider-context.md\n",
  }),
  additionalFiles: () => [{
    relativePath: ".token-shrinker/aider-context.md", format: "owned-text",
    content: aiderContext(),
  }],
};

export function aiderLaunchArgs(root = "."): readonly string[] {
  return ["--config", `${root}/.token-shrinker/aider.conf.yml`];
}

export default aiderAdapter;
