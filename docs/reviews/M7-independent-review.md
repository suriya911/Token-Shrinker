# M7 independent review packet

This packet lets a reviewer reproduce the milestone without relying on the author's machine state.

## Clean-room procedure

1. Clone the default branch into a new directory.
2. Activate the versions in `rust-toolchain.toml`, `.node-version`, and `package.json`.
3. Run `pnpm install --frozen-lockfile` and `pnpm check`.
4. Install `packages/vscode/dist/token-shrinker.vsix` into a clean VS Code profile.
5. In an untrusted fixture workspace, confirm only **Show Status** runs; context and statistics must remain blocked.
6. Trust the workspace, set `tokenShrinker.binaryPath` to the release executable, and run all three commands.
7. Inspect `docs/threat-model.md`, `SECURITY.md`, the native stdio transport, updater signature tests, and `benchmarks/public-demo.json`.

## Expected evidence

| Gate | Expected result |
|---|---|
| Windows, macOS, Linux CI | All checks pass |
| Native transport | Exact executable launch; no shell or proxy endpoint |
| Untrusted workspace | Repository-reading commands are blocked |
| Optional providers disabled | Public demo still passes |
| Context reduction | At least 30% |
| Required-evidence recall | At least 95% |
| Citation correctness | 100% |
| Supply chain | Pinned actions, advisory/license checks, secret scan, SBOM |

Record the reviewed commit, platform, VS Code version, commands run, and any findings in the release review. Automated CI supplies the repeatable clean-environment portion; a release owner must still obtain human sign-off before publishing a VSIX or stable release.
