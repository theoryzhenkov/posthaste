//! iai-callgrind regression gate for the offline store hot paths.
//!
//! Callgrind instruction counts are deterministic, so this gate gives reliable
//! pass/fail signal on noisy shared CI runners where wall-clock benches cannot.
//! `soft_limits` fails the run when any benchmark's instruction count regresses
//! beyond the threshold versus the saved baseline (see `profile.yml`).
//!
//! Requires Valgrind and `iai-callgrind-runner` at runtime; compilation does not.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use posthaste_bench::workloads;
use posthaste_domain_service::{MessagePage, SyncBatch};

/// Smaller than the Criterion population: Callgrind instruments execution and is
/// ~50x slower, and instruction counts are scale-stable for regression signal.
const COUNT: usize = 1_000;

fn ingest_setup() -> (workloads::Fixture, SyncBatch) {
    (
        workloads::open_empty(),
        workloads::sync_batch(workloads::synthetic_messages(COUNT)),
    )
}

fn seeded() -> workloads::Fixture {
    workloads::open_seeded(COUNT)
}

#[library_benchmark]
#[bench::default(setup = ingest_setup)]
fn ingest((fixture, batch): (workloads::Fixture, SyncBatch)) {
    workloads::apply_batch(black_box(&fixture), black_box(&batch));
}

#[library_benchmark]
#[bench::default(setup = seeded)]
fn list_inbox(fixture: workloads::Fixture) -> MessagePage {
    workloads::list_inbox(black_box(&fixture))
}

#[library_benchmark]
#[bench::default(setup = seeded)]
fn search(fixture: workloads::Fixture) -> MessagePage {
    workloads::search(black_box(&fixture))
}

#[library_benchmark]
#[bench::default(setup = seeded)]
fn mutate(fixture: workloads::Fixture) {
    workloads::mutate(black_box(&fixture), 1);
}

fn gate_config() -> LibraryBenchmarkConfig {
    let mut callgrind = Callgrind::default();
    // Fail the run if total instructions (Ir) regress by more than 5% vs baseline.
    callgrind.soft_limits([(EventKind::Ir, 5.0)]);
    let mut config = LibraryBenchmarkConfig::default();
    config.tool(callgrind);
    config
}

library_benchmark_group!(
    name = store;
    benchmarks = ingest, list_inbox, search, mutate
);

main!(config = gate_config(); library_benchmark_groups = store);
