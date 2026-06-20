//! Criterion wall-clock benchmarks for the offline store hot paths.
//!
//! These produce HTML reports and JSON estimates under `target/criterion` for
//! exploration and trend-watching. Deterministic regression *gating* lives in
//! the iai-callgrind bench (`store_iai`), which is immune to runner noise.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use posthaste_bench::workloads;

fn store_benches(c: &mut Criterion) {
    let count = workloads::DEFAULT_MESSAGE_COUNT;

    c.bench_function("store/ingest", |b| {
        b.iter_batched(
            || workloads::sync_batch(workloads::synthetic_messages(count)),
            |batch| {
                let fixture = workloads::open_empty();
                workloads::apply_batch(&fixture, &batch);
            },
            BatchSize::SmallInput,
        )
    });

    let seeded = workloads::open_seeded(count);

    c.bench_function("store/list_inbox", |b| {
        b.iter(|| workloads::list_inbox(&seeded))
    });
    c.bench_function("store/search", |b| b.iter(|| workloads::search(&seeded)));
    c.bench_function("store/fts_search", |b| {
        b.iter(|| workloads::fts_search(&seeded))
    });
    c.bench_function("store/mutate", |b| {
        let mut index = 0usize;
        b.iter(|| {
            workloads::mutate(&seeded, index % count);
            index += 1;
        })
    });

    let session = workloads::open_seeded(count);
    c.bench_function("store/session_loop", |b| {
        b.iter(|| workloads::session_loop(&session, workloads::DEFAULT_SESSION_ROUNDS))
    });
}

criterion_group!(benches, store_benches);
criterion_main!(benches);
