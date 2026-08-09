# Public benchmark artifacts

`public-demo.json` is raw machine-readable output from the deterministic built-ins-only demo.
`public-demo.md` is rendered from the same run. Regenerate both from the repository root:

```text
cargo run --release -p token-shrinker-cli -- benchmark demo --output benchmarks/public-demo.json --json
```

The acceptance gate compares raw and optimized context with the same labeled conservative counter,
requires at least 30% reduction, 95% required-evidence recall, 100% citation correctness, the
expected root cause, and exclusion of the synthetic secret canary. Latency samples are descriptive,
not a cross-machine regression threshold.
