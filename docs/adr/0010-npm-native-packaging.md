# ADR 0010: npm platform packages without an alpha downloader

- Status: Accepted
- Date: 2026-08-07
- Deciders: Project owner

## Context

Agent and editor users expect npm installation without a Rust compiler. A single package containing all native targets is large, while downloading an executable during installation creates reliability and supply-chain concerns.

## Decision

Publish a small umbrella CLI and exact-version platform packages selected through npm optional dependencies. The JavaScript launcher only resolves the native binary and forwards arguments, stdio, signals, and exit codes. Alpha releases do not download executables at install time or runtime. A future verified download fallback requires a separate security review and ADR amendment.

## Consequences

### Benefits

- Normal npm experience without build tools.
- Small platform-specific installs and offline-capable artifacts.
- No duplicate CLI implementation in JavaScript.

### Costs and risks

- Coordinated publication across many packages is failure-prone.
- npm optional-dependency behavior needs platform smoke tests.
- Coordinated artifacts must exist before the umbrella package can be published.

## Alternatives considered

- Compile during npm install: slow and requires a complete Rust toolchain.
- One package with every binary: simple selection, excessive download size.
- Download-only postinstall: fewer packages, weaker offline and registry integrity properties.

## Follow-up decisions

- Advertise only the platform targets that protected CI can build and smoke-test continuously.
- Revisit a download fallback only if platform packages prove insufficient and its threat model is accepted.

## Acceptance evidence

- Clean install, offline install, signal forwarding, and uninstall tests on every advertised platform.
- Tarball-content and package-version consistency checks.
