use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use token_shrinker_router::{RouterConfig, route};
use token_shrinker_types::{RouteOperation, RouteRequest, RouteScope};

fn route_benchmark(criterion: &mut Criterion) {
    let request = RouteRequest {
        operations: vec![RouteOperation::Investigation, RouteOperation::Lookup],
        scope: Some(RouteScope::Repository),
        ..RouteRequest::default()
    };
    let config = RouterConfig::default();

    criterion.bench_function("route_deep_request", |bencher| {
        bencher.iter(|| route(black_box(&request), black_box(config)));
    });
}

criterion_group!(benches, route_benchmark);
criterion_main!(benches);
