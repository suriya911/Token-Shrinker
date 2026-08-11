# Publishing runbook

Public release is intentionally split into preparation, release candidate, soak, and promotion. Never publish from a developer laptop as the default path.

## One-time ownership setup

The release owner must complete these account-bound steps:

1. Reserve the npm `@token-shrinker` scope and all public package names.
2. Create each npm package once if required, then configure GitHub Actions trusted publishing for this repository, the exact release workflow filename, and the protected `release` environment. Prefer OIDC over a long-lived npm token.
3. Create or verify the immutable VS Code Marketplace publisher ID `token-shrinker`. Store Marketplace authentication only as a protected CI secret until Microsoft Entra automated publishing replaces PAT-based publishing.
4. Enable GitHub tag protection and required reviewers for the `release` environment.
5. Confirm the repository URL, private vulnerability reporting, support owner, rollback owner, and signing identity.
6. Decide whether Rust crates are in v1 scope. They are not currently advertised as a public install surface.

Do not paste npm, GitHub, Microsoft, or signing credentials into repository files, issues, chat, logs, or command arguments.

## Trusted npm staged publishing

The tag workflow is `.github/workflows/publish-npm.yml`. It uses GitHub Actions OIDC and the protected `release` environment; it must not receive an `NPM_TOKEN` or `NODE_AUTH_TOKEN` secret.

Configure the same trusted publisher on all six npm packages:

- GitHub organization or user: `suriya911`
- Repository: `Token-Shrinker`
- Workflow filename: `publish-npm.yml`
- Environment: `release`
- Allowed action: staged publishing only

Packages:

- `@token-shrinker/cli-linux-x64-gnu`
- `@token-shrinker/cli-darwin-arm64`
- `@token-shrinker/cli-darwin-x64`
- `@token-shrinker/cli-win32-x64`
- `@token-shrinker/sdk`
- `@token-shrinker/cli`

Pushing a protected `v*` tag starts the workflow. The tag must equal `v` followed by the coordinated package version. The workflow verifies the complete release gate, stages all four native packages on their matching operating systems, then stages the SDK and umbrella CLI. It skips an exact version that is already public, so rerunning a completed tag is safe and cannot overwrite an npm version.

After every job passes, inspect and approve the staged packages in this order: all four native packages, SDK, then CLI. Promote npm's `latest` tag only after a clean public installation verifies that the umbrella CLI resolves and launches the native binary on every supported platform.

## Release-candidate sequence

1. Choose a coordinated version such as `1.0.0-rc.1`. VS Code Marketplace does not accept SemVer prerelease suffixes, so use a distinct numeric extension version and publish it with `vsce --pre-release`.
2. Update the Cargo workspace and every public npm/VS Code manifest together; regenerate the lockfiles.
3. Fill the changelog date and migration notes.
4. Run `pnpm check` and `node scripts/verify-release-metadata.mjs --publish` with the release-owner acknowledgment set only in the protected environment.
5. Build every native target on its matching GitHub-hosted runner. Generate SHA-256 checksums, CycloneDX SBOMs, and GitHub artifact attestations.
6. Install each tarball/VSIX into a clean target and run version, doctor, agent add/remove, offline install, and uninstall tests.
7. Publish native npm packages first, then `@token-shrinker/cli` and `@token-shrinker/sdk`, all to the non-default `next` dist-tag.
8. Publish the VS Code build as pre-release. Do not reuse that numeric version for the stable extension.
9. Verify the public npm provenance, package contents, checksums, attestations, and Marketplace identity from a clean machine.
10. Run the full benchmark and 24-hour daemon soak. Record memory growth, crashes, reconnect behavior, and exact commit/artifact hashes.

## Stable promotion

After maintainer approval and a passing soak, build stable artifacts from the protected `v1.0.0` tag. Re-run clean installation and verification, publish the distinct stable VS Code version, then move npm's `latest` dist-tag only after every referenced artifact is public and verified. Publish the signed compatibility manifest last.

## Rollback

- npm: move `latest` back to the last verified version; deprecate the affected release with a precise reason. Unpublish only when policy permits and incident response requires it.
- VS Code: unpublish only for a severe incident; otherwise ship a higher fixed version and document disabling/uninstalling the affected release.
- GitHub: keep compromised artifacts and attestations out of the promoted release, revoke affected credentials/identities, and publish an incident note.
- Compatibility manifest: stop advertising the affected version and sign a new non-downgrading manifest. The v1 updater only reports; it never activates artifacts.

The final public-install gate is complete only after npm, GitHub, Marketplace, and all four supported OS/architecture combinations have been independently verified.
