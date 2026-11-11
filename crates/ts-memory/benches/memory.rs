//! `BUILD_PLAN` section 15 suite 4: `SQLite` search, write, and retention at increasing record counts.
//!
//! The point of scaling the record count is to expose growth that a single-size benchmark hides.
//! Each search case also asserts it still finds the record it planted, so a regression that indexes
//! nothing cannot post a fast time.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tempfile::TempDir;
use token_shrinker_memory::{MemoryScope, MemoryStore, NewMemory};

const RECORD_COUNTS: [usize; 3] = [100, 1_000, 10_000];
/// Planted in every store so a search that returns nothing fails loudly.
const NEEDLE: &str = "authorization session expiry boundary";

fn new_memory(index: usize, content: String) -> NewMemory {
    NewMemory {
        id: format!("bench-{index:07}"),
        scope: MemoryScope::User,
        content,
        created_at_unix_ms: 1_700_000_000_000 + i64::try_from(index).expect("benchmark index"),
        expires_at_unix_ms: None,
    }
}

/// Builds a populated store on disk. Returns the directory so it outlives the store.
fn populated_store(records: usize) -> (TempDir, MemoryStore) {
    let directory = TempDir::new().expect("benchmark temp dir");
    let mut store =
        MemoryStore::open(directory.path().join("memory.db")).expect("benchmark memory store");
    for index in 0..records {
        let content = if index == records / 2 {
            NEEDLE.to_owned()
        } else {
            format!("routine note {index} about unrelated build output and cache state")
        };
        store
            .remember(&new_memory(index, content))
            .expect("benchmark remember");
    }
    (directory, store)
}

fn write_benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("memory_write");
    for records in RECORD_COUNTS {
        let (_directory, mut store) = populated_store(records);
        let mut counter = records;
        group.bench_function(format!("remember_into_{records}_records"), |bencher| {
            bencher.iter(|| {
                counter += 1;
                store
                    .remember(black_box(&new_memory(
                        counter,
                        "appended benchmark memory".to_owned(),
                    )))
                    .expect("benchmark remember");
            });
        });
    }
    group.finish();
}

fn search_benchmark(criterion: &mut Criterion) {
    let scope = MemoryScope::User;
    let now = 1_800_000_000_000_i64;
    let mut group = criterion.benchmark_group("memory_search");
    for records in RECORD_COUNTS {
        let (_directory, store) = populated_store(records);
        let found = store
            .search(&scope, "authorization", now, 10)
            .expect("benchmark search");
        assert!(
            found.iter().any(|record| record.content == NEEDLE),
            "search over {records} records lost the planted record"
        );
        group.bench_function(format!("search_{records}_records"), |bencher| {
            bencher.iter(|| {
                store
                    .search(
                        black_box(&scope),
                        black_box("authorization"),
                        black_box(now),
                        black_box(10),
                    )
                    .expect("benchmark search")
            });
        });
    }
    group.finish();
}

/// Builds a store in which every record has already expired, so a prune does the maximum work.
fn expired_store(records: usize) -> (TempDir, MemoryStore) {
    let directory = TempDir::new().expect("benchmark temp dir");
    let mut store =
        MemoryStore::open(directory.path().join("memory.db")).expect("benchmark memory store");
    for index in 0..records {
        // The expiry must sit after this record's own creation time, which new_memory staggers
        // by index, and before the prune instant used below.
        store
            .remember(&NewMemory {
                expires_at_unix_ms: Some(
                    1_700_000_100_000 + i64::try_from(index).expect("benchmark index"),
                ),
                ..new_memory(index, format!("expiring note {index}"))
            })
            .expect("benchmark remember");
    }
    (directory, store)
}

fn retention_benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("memory_retention");
    // A prune is destructive: the first call empties the store and every later call measures an
    // empty table instead of the deletion. Each iteration therefore gets a freshly populated
    // store, whose construction criterion excludes from the timing. That setup costs one durable
    // write per record, so these counts stay below the ones the read-only groups use.
    for records in [100_usize, 500, 1_000] {
        group.bench_function(format!("prune_expired_over_{records}_records"), |bencher| {
            bencher.iter_batched(
                || expired_store(records),
                |(_directory, mut store)| {
                    let removed = store
                        .prune_expired(black_box(1_800_000_000_000))
                        .expect("benchmark prune");
                    assert_eq!(removed, records, "prune must remove every expired record");
                },
                criterion::BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn integrity_benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("memory_integrity");
    for records in RECORD_COUNTS {
        let (_directory, store) = populated_store(records);
        group.bench_function(format!("integrity_check_{records}_records"), |bencher| {
            bencher.iter(|| store.integrity_check().expect("benchmark integrity check"));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    write_benchmark,
    search_benchmark,
    retention_benchmark,
    integrity_benchmark
);
criterion_main!(benches);
