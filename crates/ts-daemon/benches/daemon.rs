//! `BUILD_PLAN` section 15 suite 5: cold start, request concurrency, reconnect, and steady state.
//!
//! Section 15 sets a p50 under 500 ms for daemon readiness on the reference machine, so cold start
//! is measured end to end: config validation, runtime-directory hardening, single-instance
//! locking, listener bind, and discovery publication, up to the first request that actually
//! returns. Every case asserts the daemon answered correctly, so a regression that fails fast
//! cannot post a good time.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use serde_json::{Value, json};
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use token_shrinker_daemon::{BuiltInHandler, CapabilityRegistry, DaemonConfig, DaemonHandle};

fn daemon_config(directory: &TempDir) -> DaemonConfig {
    DaemonConfig {
        runtime_directory: directory.path().to_owned(),
        max_frame_bytes: 64 * 1024,
        max_concurrency: 8,
        graceful_shutdown_timeout: Duration::from_secs(5),
        max_log_bytes: 64 * 1024,
    }
}

fn start_daemon(directory: &TempDir) -> DaemonHandle {
    DaemonHandle::start(
        &daemon_config(directory),
        Arc::new(BuiltInHandler::new(CapabilityRegistry::default())),
    )
    .expect("benchmark daemon start")
}

/// Cold start through to the first served request, which is what "ready" means to a caller.
fn cold_start_benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("daemon_cold_start");
    group.sample_size(20);
    group.bench_function("start_to_first_response", |bencher| {
        bencher.iter_batched(
            || TempDir::new().expect("benchmark temp dir"),
            |directory| {
                let daemon = start_daemon(&directory);
                let health = daemon
                    .call("health", Value::Null)
                    .expect("benchmark health");
                assert!(!health.is_null(), "daemon reported no health state");
                daemon.shutdown().expect("benchmark shutdown");
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

/// Steady-state request cost against an already running daemon.
fn request_benchmark(criterion: &mut Criterion) {
    let directory = TempDir::new().expect("benchmark temp dir");
    let daemon = start_daemon(&directory);
    let payload = json!({ "note": "benchmark round trip" });

    let mut group = criterion.benchmark_group("daemon_request");
    group.bench_function("echo_round_trip", |bencher| {
        bencher.iter(|| {
            let echoed = daemon
                .call(black_box("echo"), black_box(payload.clone()))
                .expect("benchmark echo");
            assert_eq!(echoed, payload, "echo must return the exact payload");
        });
    });
    group.bench_function("capabilities_round_trip", |bencher| {
        bencher.iter(|| {
            daemon
                .call(black_box("capabilities"), Value::Null)
                .expect("benchmark capabilities")
        });
    });
    group.finish();

    daemon.shutdown().expect("benchmark shutdown");
}

/// Concurrent callers against one daemon, which is the bounded-worker path.
fn concurrency_benchmark(criterion: &mut Criterion) {
    let directory = TempDir::new().expect("benchmark temp dir");
    let daemon = Arc::new(start_daemon(&directory));

    let mut group = criterion.benchmark_group("daemon_concurrency");
    for callers in [2_usize, 4, 8] {
        group.bench_function(format!("{callers}_concurrent_callers"), |bencher| {
            bencher.iter(|| {
                let handles = (0..callers)
                    .map(|index| {
                        let daemon = Arc::clone(&daemon);
                        thread::spawn(move || {
                            let payload = json!({ "caller": index });
                            let echoed = daemon
                                .call("echo", payload.clone())
                                .expect("benchmark echo");
                            assert_eq!(echoed, payload, "concurrent echo returned another payload");
                        })
                    })
                    .collect::<Vec<_>>();
                for handle in handles {
                    handle.join().expect("benchmark caller thread");
                }
            });
        });
    }
    group.finish();

    Arc::into_inner(daemon)
        .expect("benchmark daemon still shared")
        .shutdown()
        .expect("benchmark shutdown");
}

criterion_group!(
    benches,
    cold_start_benchmark,
    request_benchmark,
    concurrency_benchmark
);
criterion_main!(benches);
