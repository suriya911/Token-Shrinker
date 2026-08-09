use std::collections::BTreeMap;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use semver::VersionReq;
use token_shrinker_provider::{
    ProviderError, ProviderLimits, ProviderQuality, ProviderSpec, resolve_with_fallback,
    validate_version,
};

fn provider_contract(criterion: &mut Criterion) {
    let requirement = VersionReq::parse(">=0.9.0, <1.0.0").expect("requirement");
    criterion.bench_function("provider/incremental/version_validation", |bencher| {
        bencher.iter(|| validate_version(black_box("graphify 0.9.35"), &requirement));
    });

    let quality = ProviderQuality {
        raw_tokens: 10_000,
        optimized_tokens: 3_500,
        relevant_total: 20,
        relevant_retained: 19,
    };
    criterion.bench_function("provider/quality/reduction", |bencher| {
        bencher.iter(|| black_box(quality).reduction_basis_points());
    });
    criterion.bench_function("provider/quality/evidence_recall", |bencher| {
        bencher.iter(|| black_box(quality).recall_basis_points());
    });

    let mut spec = ProviderSpec {
        id: "optional".to_owned(),
        command: "unused".into(),
        base_args: Vec::new(),
        environment: BTreeMap::default(),
        version_requirement: VersionReq::STAR,
        required: false,
        limits: ProviderLimits::default(),
    };
    criterion.bench_function("provider/fallback/optional", |bencher| {
        bencher.iter(|| {
            resolve_with_fallback(
                black_box(&spec),
                Err::<(), _>(ProviderError::Timeout),
                "builtin",
                || (),
            )
        });
    });
    spec.required = true;
    criterion.bench_function("provider/fallback/required", |bencher| {
        bencher.iter(|| {
            resolve_with_fallback(
                black_box(&spec),
                Err::<(), _>(ProviderError::Timeout),
                "builtin",
                || (),
            )
        });
    });
}

criterion_group!(benches, provider_contract);
criterion_main!(benches);
