# Public benchmark artifacts

`public-demo.json` is raw machine-readable output from the deterministic built-ins-only demo.
`public-demo.md` is rendered from the same run. Regenerate both from the repository root:

```text
cargo run --release -p token-shrinker-cli -- benchmark demo --output benchmarks/public-demo.json --json
```

The report uses schema v2. Its `reproducibility` object records the source commit and
dirty state, CPU/RAM/filesystem/power metadata, Rust and Node versions, warmup/sample
counts, raw latency samples, and cache state. Host-specific values may be `unknown` or
`null` when the platform cannot report them; CI can provide overrides through the
`TOKEN_SHRINKER_BENCH_*` environment variables.

The end-to-end Criterion target exercises the same route/context/terminal path with a realistic
session-expiry debugging task. It compares the unoptimized task bundle with the bounded context
and compressed terminal result, and fails before timing if any required source, citation content,
or reduction invariant is lost:

```text
cargo bench -p token-shrinker-cli --bench end_to_end
```

The benchmark is deterministic in its fixture and assertions, but latency numbers remain
machine-specific. Run it on documented reference hardware before publishing performance claims.

The acceptance gate compares raw and optimized context with the same labeled conservative counter,
requires at least 30% reduction, 95% required-evidence recall, 100% citation correctness, the
expected root cause, and exclusion of the synthetic secret canary. Latency samples are descriptive,
not a cross-machine regression threshold.
