//! End-to-end task-bundle benchmark.
//!
//! This measures the public demo path as a user experiences it: route a
//! multi-file debugging task, build bounded repository context, compress a
//! noisy terminal result, and compare it with the unoptimized bundle. The
//! required files are checked before timing so a faster benchmark cannot hide
//! an evidence-loss regression.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Duration,
};
use token_shrinker_compress::{TerminalInput, compress_terminal};
use token_shrinker_context::{ConservativeEstimator, TokenCounter};
use token_shrinker_daemon::build_baseline_context;
use token_shrinker_router::{RouterConfig, route};
use token_shrinker_types::{RouteOperation, RouteRequest, RouteScope, TokenBudget};

const TASK: &str = "Find why a session expiring exactly at request time is authorized and propose the smallest safe fix.";
const REQUIRED: &[&str] = &[
    "tests/session.test.mjs",
    "src/api/authorize.mjs",
    "src/auth/session.mjs",
    "config/auth-policy.json",
];
const BASELINE: &[&str] = &[
    "README.md",
    "config/auth-policy.json",
    "docs/session-policy.md",
    "docs/generated/session-policy-copy.md",
    "logs/noisy-test-output.log",
    "src/api/authorize.mjs",
    "src/auth/session.mjs",
    "src/billing/invoice.mjs",
    "src/notifications/digest.mjs",
    "tests/session.test.mjs",
];

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("demo-repo")
}

fn query() -> String {
    format!("{TASK} session.test authorize auth-policy session.mjs boundary assertion caller")
}

fn baseline_tokens(root: &Path, estimator: &ConservativeEstimator) -> u64 {
    BASELINE
        .iter()
        .map(|relative| {
            estimator
                .count(&fs::read_to_string(root.join(relative)).expect("fixture file"))
                .tokens()
        })
        .sum()
}

fn optimized_bundle(root: &Path) -> (u64, u64) {
    let decision = route(
        &RouteRequest {
            operations: vec![RouteOperation::Debug],
            scope: Some(RouteScope::MultiFile),
            budget_override: Some(TokenBudget::from_u32(4_000).expect("benchmark budget")),
            ..RouteRequest::default()
        },
        RouterConfig::default(),
    );
    let context = build_baseline_context(root, &query(), decision.budget).expect("context build");
    let selected = context
        .bundle
        .items
        .iter()
        .filter(|item| {
            let uri = item.location.uri.replace('\\', "/");
            REQUIRED.contains(&uri.as_str())
        })
        .collect::<Vec<_>>();
    let retained = selected
        .iter()
        .map(|item| item.location.uri.replace('\\', "/"))
        .collect::<Vec<_>>();
    assert_eq!(
        retained.len(),
        REQUIRED.len(),
        "optimized context lost required evidence"
    );
    assert!(
        REQUIRED
            .iter()
            .all(|required| retained.iter().any(|value| value == required)),
        "optimized context did not retain every required file"
    );
    assert!(
        selected
            .iter()
            .all(|item| fs::read_to_string(root.join(&item.location.uri))
                .is_ok_and(|source| source == item.content)),
        "citation content changed"
    );

    let terminal =
        fs::read_to_string(root.join("logs/noisy-test-output.log")).expect("terminal fixture");
    let compressed = compress_terminal(
        &TerminalInput {
            command: vec![
                "node".into(),
                "--test".into(),
                "tests/session.test.mjs".into(),
            ],
            exit_code: Some(1),
            duration: Duration::ZERO,
            stdout: String::new(),
            stderr: terminal,
            truncated: false,
            raw_handle: None,
        },
        4,
        4,
    );
    let optimized_context = selected.iter().map(|item| item.token_count.tokens()).sum();
    let optimized_terminal = estimator_tokens(&compressed);
    (optimized_context, optimized_terminal)
}

fn estimator_tokens(value: &impl serde::Serialize) -> u64 {
    ConservativeEstimator
        .count(&serde_json::to_string(value).expect("serialize benchmark value"))
        .tokens()
}

fn end_to_end_benchmark(criterion: &mut Criterion) {
    let root = fixture_root();
    let estimator = ConservativeEstimator;
    let baseline = baseline_tokens(&root, &estimator);
    let optimized = optimized_bundle(&root);
    let baseline_total = baseline
        + estimator
            .count(
                &fs::read_to_string(root.join("logs/noisy-test-output.log"))
                    .expect("terminal fixture"),
            )
            .tokens();
    let optimized_total = optimized.0 + optimized.1;
    assert!(
        optimized_total < baseline_total,
        "optimized bundle must be smaller"
    );
    let reduction_basis_points = baseline_total
        .saturating_sub(optimized_total)
        .saturating_mul(10_000)
        / baseline_total.max(1);
    assert!(
        reduction_basis_points >= 3_000,
        "optimized task bundle reduction is below 30%"
    );

    let mut group = criterion.benchmark_group("end_to_end_task_bundle");
    group.bench_function("baseline_token_count", |bencher| {
        bencher.iter(|| baseline_tokens(black_box(&root), black_box(&estimator)))
    });
    group.bench_function("optimized_route_context_compress", |bencher| {
        bencher.iter_batched(
            || root.clone(),
            |root| optimized_bundle(black_box(&root)),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, end_to_end_benchmark);
criterion_main!(benches);
