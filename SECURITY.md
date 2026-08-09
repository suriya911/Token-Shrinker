# Security Policy

## Supported versions

Token-Shrinker is pre-alpha and has no supported release yet. Until the first release, security fixes are made only on the default development branch.

| Version | Supported |
|---|---|
| Default branch | Yes, best effort |
| Unreleased local snapshots | No guarantee |

## Reporting a vulnerability

Do not open a public issue, discussion, or pull request for a suspected vulnerability.

Use the repository's GitHub private vulnerability reporting feature. If it is unavailable, contact the repository owner privately through their GitHub profile. Never include vulnerability details in a public issue.

Include only the minimum information needed to reproduce the problem:

- affected commit or version;
- platform and configuration;
- impact and attack prerequisites;
- deterministic reproduction steps or a minimal proof of concept;
- suggested mitigation, if known.

Do not include real credentials, customer data, proprietary repositories, private prompts, or unrelated system information. Use synthetic canaries and fixtures.

## Response targets

These are targets, not a service-level agreement:

- acknowledge a complete report within 3 business days;
- provide an initial severity assessment within 7 business days;
- coordinate a fix and disclosure timeline based on impact;
- credit reporters who request credit and follow coordinated disclosure.

## Security scope

High-priority areas include command execution, path traversal and symlink escape, secret persistence, local IPC authorization, provider isolation, configuration editing, archive handling, updater verification and rollback, and changes to an agent's native model transport.

The baseline runtime is local-first and has no hosted telemetry endpoint. Optional integrations may add outbound data paths; each must disclose its data boundary and remain disabled or safely degraded when unavailable.

The maintained trust-boundary analysis, abuse cases, mitigations, residual risks, and verification map are documented in [`docs/threat-model.md`](docs/threat-model.md).
