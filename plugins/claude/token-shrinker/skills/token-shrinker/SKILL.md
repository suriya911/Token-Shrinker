---
name: token-shrinker
description: Use Token-Shrinker to route work, build bounded evidence context, fetch omitted sources, inspect warnings, and request approved execution.
license: Apache-2.0
---
<!-- token-shrinker-owned:v1 -->

# Token-Shrinker workflow

1. Call `token_shrinker_capabilities` before relying on optional providers or execution.
2. Call `token_shrinker_route` when the appropriate FAST, BUILD, or DEEP route is unclear.
3. Call `token_shrinker_build_context` with the repository root, concrete goal, and explicit token budget.
4. Treat only returned bundle item content as evidence. A source ID or omission is not evidence by itself.
5. If an omitted source may be required for correctness, call `token_shrinker_fetch_source` with its exact source ID before answering. If retrieval fails or the content still does not establish the claim, say the claim is unverified.
6. Cite only content actually returned by `token_shrinker_build_context` or `token_shrinker_fetch_source`. Never infer implementation details from filenames, source IDs, package metadata, or prior knowledge.
7. Inspect every warning and preserve relevant warnings, citations, commands, exact errors, and uncertainty.
8. Call `token_shrinker_execute` only after the user approves the exact command and policy permits it.
9. Use `token_shrinker_format_final` only for the final human response. Never apply concise formatting to tool calls, JSON, code, commands, logs, citations, or intermediate evidence.

If Token-Shrinker is unavailable, report the MCP error and run `token_shrinker_capabilities` again after checking the plugin installation. Never redirect provider endpoints, credentials, or native model transport.
