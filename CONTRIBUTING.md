# Contributing to Token-Shrinker

Token-Shrinker is pre-alpha. Contributions are welcome, but public contracts and provider boundaries are still under review.

## Before starting

- Search existing issues and architecture decision records (ADRs).
- Open an issue before changing a public protocol, provider trait, security boundary, update behavior, or release invariant.
- Do not introduce a required hosted service or optional-tool dependency.
- Never submit credentials, private prompts, source captures, or production logs.

## Development setup

Install Git, Node.js 24, pnpm 11, and rustup. The repository pins the exact Rust toolchain and pnpm version.

```bash
pnpm install --frozen-lockfile
pnpm check
```

`pnpm check` runs Rust formatting, Clippy with warnings denied, Rust tests, and every package's lint, typecheck, test, and build scripts.

## Making a change

1. Start from a clean branch.
2. Keep the change to one coherent behavior.
3. Add a failing test or deterministic fixture first when practical.
4. Preserve provenance, native agent transport, local-first defaults, and safe fallbacks.
5. Update schemas, generated types, documentation, changelog, and ADRs when contracts change.
6. Run targeted checks, then `pnpm check`.
7. Include benchmark evidence for token, latency, memory, ranking, or compression claims.

Use Conventional Commit subjects where practical, such as `feat(router): add explicit mode override` or `fix(context): retain diagnostic evidence`.

## Pull requests

A pull request should explain:

- the problem and observable behavior;
- why this design fits the accepted ADRs;
- tests and commands run;
- security, privacy, compatibility, and migration effects;
- benchmark changes when relevant;
- remaining risks or follow-up work.

Pull requests must not weaken execution approval, allowed-root enforcement, redaction, IPC isolation, update verification, or native model transport.

## Licensing

Unless explicitly stated otherwise, contributions intentionally submitted for inclusion are provided under the Apache License 2.0, consistent with section 5 of the project license.

## Community

Participation is governed by [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md). Security vulnerabilities follow [SECURITY.md](./SECURITY.md), not the public issue tracker.

