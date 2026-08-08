use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use token_shrinker_context::{
    ConservativeEstimator, ContentHash, ContextCandidate, RelevanceSignals, Sensitivity, SourceId,
    SourceKind, SourceLocation, build_context,
};
use token_shrinker_types::TokenBudget;

fn candidates() -> Vec<ContextCandidate> {
    (0_u32..100)
        .map(|index| ContextCandidate {
            source_id: SourceId::new(format!("repo:src/file-{index}.rs"))
                .expect("benchmark source ID"),
            source_kind: SourceKind::RepositoryFile,
            location: SourceLocation {
                uri: format!("src/file-{index}.rs"),
                start_line: None,
                end_line: None,
            },
            content: format!("fn function_{index}() {{ value_{index} }}"),
            content_hash: ContentHash::new(format!("{index:064x}"))
                .expect("benchmark content hash"),
            sensitivity: Sensitivity::Public,
            modified_unix_ms: None,
            relevance: RelevanceSignals {
                exact_match: index % 10 == 0,
                path_match: index % 5 == 0,
                diagnostic: index % 25 == 0,
                freshness: u8::try_from(index).expect("benchmark index fits u8"),
            },
        })
        .collect()
}

fn rank_and_pack_benchmark(criterion: &mut Criterion) {
    let candidates = candidates();
    let budget = TokenBudget::from_u32(2_048).expect("benchmark budget");
    let estimator = ConservativeEstimator;

    criterion.bench_function("rank_and_pack_100_candidates", |bencher| {
        bencher.iter(|| {
            build_context(
                black_box(&candidates),
                black_box(budget),
                black_box(&estimator),
            )
        });
    });
}

criterion_group!(benches, rank_and_pack_benchmark);
criterion_main!(benches);
