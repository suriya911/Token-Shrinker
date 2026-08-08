# Token-Shrinker demo repository

This deterministic fixture contains an intentional session-expiration boundary bug. It also contains plausible distractors, duplicated generated documentation, noisy terminal output, and a synthetic secret canary.

Run the failing test from the Token-Shrinker repository root:

```bash
node --test fixtures/demo-repo/tests/session.test.mjs
```

The test must fail until the fixture is deliberately copied into an isolated benchmark run and fixed there. Do not repair the committed fixture.
