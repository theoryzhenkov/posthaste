//! M23b (D63 completion) — production store callers route the sync hot path
//! through the tokio blocking pool. `super::offload` (`service.rs`) is the
//! seam every async call site wraps its `SyncWriteStore`/`MessageCommandStore`
//! call in (see `ServiceSyncSink::emit`, `apply_assertion_to_canonical`, the
//! outbox settle write, lazy body caching).
//!
//! This proves the gate directly on `offload` rather than on a specific call
//! site: a blocking closure run through `offload` does not occupy the async
//! worker it was called from — a concurrent task on the *same* single-worker
//! runtime completes while the closure is still in flight, which would be
//! impossible if `offload` ran it inline. Mirrors the M23 store-level proof
//! (`posthaste-store/src/tests/concurrency.rs`:
//! `slow_write_does_not_block_concurrent_read`) at the domain-service seam.

use std::time::Duration;

use tokio::sync::oneshot;

use crate::service::offload;
use posthaste_domain_model::StoreError;

/// `worker_threads = 1`: the strongest version of the proof. If `offload`
/// merely called the closure inline (the D63/M23b anti-pattern), the sole
/// async worker would be occupied for the whole blocking duration and the
/// concurrent task below could never run until it was released — the test
/// would hang and time out.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn offload_does_not_block_a_concurrent_task_on_the_same_single_worker_runtime() {
    // `blocking_open`: the offloaded closure signals it has started (and is
    // now parked on the blocking pool) via a real blocking channel receive —
    // not a wall-clock sleep — so the test is deterministic, not timing-based.
    let (blocking_open_tx, blocking_open_rx) = oneshot::channel::<()>();
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let offloaded = tokio::spawn(async move {
        offload(move || {
            let _ = blocking_open_tx.send(());
            // Blocks the blocking-pool thread, never the async worker — the
            // whole point of `offload`.
            let _ = release_rx.blocking_recv();
            Ok::<(), StoreError>(())
        })
        .await
    });

    // Wait — without blocking the async worker — until the offloaded closure
    // is in flight.
    blocking_open_rx
        .await
        .expect("offloaded closure should signal it started");

    // With the offload still in flight (holding the only worker thread
    // hostage if `offload` ran inline instead of on the blocking pool), a
    // concurrent task on this same runtime must still complete promptly.
    let concurrent = tokio::time::timeout(Duration::from_secs(5), async { 21 + 21 })
        .await
        .expect("a concurrent task must make progress while the closure is offloaded");
    assert_eq!(
        concurrent, 42,
        "the concurrent task should run to completion, not just be scheduled",
    );

    // Release the offloaded closure and confirm it completed cleanly.
    let _ = release_tx.send(());
    offloaded
        .await
        .expect("the spawned task should not panic")
        .expect("offload should propagate the closure's Ok result");
}

/// A panic inside the offloaded closure is caught (by `spawn_blocking`) and
/// surfaced as a `StoreError`, not a silent hang or an unhandled panic that
/// takes down the caller.
#[tokio::test(flavor = "multi_thread")]
async fn offload_surfaces_a_panic_in_the_closure_as_a_store_error() {
    let result: Result<(), StoreError> = offload(|| panic!("boom")).await;
    match result {
        Err(StoreError::Failure(message)) => {
            assert!(
                message.contains("store write task failed"),
                "unexpected message: {message}",
            );
        }
        other => panic!("expected a StoreError::Failure, got {other:?}"),
    }
}
