# Migration guide

This guide covers moving between published Token-Shrinker versions. Each section lists
required actions, observable behavior changes, and anything that is safe to ignore.

Released versions to date are `0.1.0`, `0.1.1`, `0.1.2`, `0.1.3`, and `0.2.0`. All are pre-`1.0` alpha releases.
Until `1.0.0` ships, the compatibility guarantees in [Compatibility policy](#compatibility-policy)
describe intent rather than a frozen contract.

## 0.1.3 to 0.2.0

`0.2.0` adds additive MCP tools for project task continuity and exact token measurements.
The protocol and MCP protocol versions remain `1.0` and `2025-11-25`; existing tools and
telemetry schemas remain readable. Estimated measurements stay separate from exact
measurements by tokenizer identity. Update the CLI and refresh agent plugins before use.

```bash
npm install --global @token-shrinker/cli@0.2.0
```

After upgrading, verify `token_shrinker_capabilities`, `token_shrinker_task_status`, and
`token_shrinker_stats`. No native model-provider endpoints or credentials are changed.

## 0.1.2 to 0.1.3

`0.1.3` is a coordinated patch release. There are no protocol, schema, database, or
configuration migrations. Update the installed surface you use and verify the reported
binary/package versions are both `0.1.3`.

```bash
npm install --global @token-shrinker/cli@0.1.3
code --install-extension token-shrinker.token-shrinker
```

Refresh the Claude Code and Codex marketplaces, reinstall the Token-Shrinker plugin, then
run `token-shrinker version --json` and `token-shrinker doctor --json`.

## 0.1.0 to 0.1.1

`0.1.1` is a patch release. There are no breaking changes, no schema migrations, and no
configuration changes. The Token-Shrinker protocol stays at `1.0` and the MCP protocol
stays at `2025-11-25`.

### Required actions

Update whichever surfaces you installed. None of them depend on the others.

```bash
# npm
npm install --global @token-shrinker/cli@0.1.1

# VS Code extension
code --install-extension token-shrinker.token-shrinker
```

For the Claude Code and Codex plugins, refresh the marketplace and reinstall the plugin as
described in the [adapter guide](../adapters/README.md). The marketplace manifests are
pinned to `0.1.1`.

Confirm the result:

```bash
token-shrinker version --json
token-shrinker doctor --json
```

Both should report package and binary version `0.1.1`, protocol `1.0`, and health `healthy`.

### Observable behavior change

Context ranking changed. Repository paths you name explicitly in a request now outrank
broad prose matches and are treated as mandatory evidence under bounded budgets. Nested
hidden files such as `.mcp.json` are discoverable and receive path-match evidence.

If you previously worked around a named file being dropped from a bundle — for example by
raising the budget or repeating the path — that workaround is no longer needed. Bundles
built from the same request may now differ from `0.1.0` output: the named file is present,
and a lower-ranked file may have been displaced to stay inside the budget. Bundle hashes
therefore are not comparable across the two versions.

### Nothing to migrate

- No SQLite memory or telemetry schema changes. Existing databases are read as-is.
- No configuration file changes. Existing `token-shrinker` config remains valid.
- No MCP tool additions, removals, or signature changes. All nine tools are unchanged.
- No adapter reinstall required. Installed adapters keep working against the new binary.

## Downgrading

Downgrading from `0.1.1` to `0.1.0` is supported. Reinstall the older version through the
same surface you used to upgrade. Because there were no schema migrations, databases
written by `0.1.1` remain readable by `0.1.0`.

The context-ranking behavior reverts with the downgrade, so explicitly named paths lose
their mandatory-evidence treatment again.

## Compatibility policy

These rules apply within a protocol major version and are the basis for the `v1.0.0`
compatibility freeze:

- New protocol fields are additive. Existing fields do not change meaning or type.
- Unknown optional capabilities are ignored safely rather than treated as errors.
- Database changes are forward-only and ship with a tested backup and restore path.
- A release cannot bypass execution policy, replace native model transport, or make an
  optional tool mandatory.

Native model-provider endpoints and credentials are never modified by installing,
upgrading, or removing Token-Shrinker or any of its adapters.

## Not yet frozen

The following are expected to change before `v1.0.0` and should not be depended on:

- Rust crate names and their public APIs. The crates are source-workspace components and
  are not published to crates.io.
- Telemetry record shapes beyond the documented content-free aggregate fields.
- Bundle hash stability across versions, as described above.

## Related documents

- [CHANGELOG.md](../../CHANGELOG.md) for the full list of changes per release.
- [Release notes 0.1.1](./RELEASE_NOTES_0.1.1.md) and [0.1.0](./RELEASE_NOTES_0.1.0.md).
- [Publishing runbook](./PUBLISHING.md) for maintainer release steps.
- [Release try-out guide](./TRY_IT.md) for clean-install verification.
