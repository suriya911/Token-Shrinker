# Migration to v1

There is no earlier stable release. This guide covers upgrades from source or `0.x` engineering builds.

1. Stop a running Token-Shrinker daemon.
2. Back up user configuration and local memory before replacing an engineering build.
3. Remove project adapters with the old executable when possible: `token-shrinker remove <agent> --root <project>`.
4. Install the coordinated v1 npm packages. Do not mix umbrella and native package versions.
5. Run `token-shrinker version --json` and confirm binary/package versions match and protocol is `1.0`.
6. Run `token-shrinker doctor --json`.
7. Re-add each project adapter and inspect the paths reported by the command.
8. Start the daemon only if the client uses local IPC; stdio MCP integrations launch their own process.

Downgrade is supported only when the older binary accepts the stored schema. If it reports a schema or protocol incompatibility, restore the backup rather than editing the database manually.
