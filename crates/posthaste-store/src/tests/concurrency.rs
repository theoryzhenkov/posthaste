//! M23 (D63) — SQLite runs off the async workers. The store's blocking rusqlite
//! work is offloaded to the tokio blocking pool via `spawn_blocking`
//! ([`DatabaseStore::write_transaction_async`] / [`DatabaseStore::read_async`]),
//! so a large/slow write no longer blocks a concurrent read on the runtime.

use super::*;
use tokio::sync::oneshot;

/// A slow write offloaded to the blocking pool must not block a concurrent
/// read: the read completes *while the write transaction is still open* (holding
/// the write mutex + an in-flight SQLite txn).
///
/// The write is held open by a channel handshake rather than a wall-clock sleep
/// in production code — and the release is only sent *after* the read has
/// already returned, so the test deterministically proves the read finished
/// while the write was in progress. The write's blocking `blocking_recv` runs on
/// the blocking pool thread, never on an async worker — the whole point of the
/// `spawn_blocking` seam.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_write_does_not_block_concurrent_read() {
    let root = crate::test_support::temp_root();
    let store = Arc::new(DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap());
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary").unwrap();
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
        "message-1",
    )
    .unwrap();

    // `write_open`: the write closure signals its txn is open (write mutex held).
    // `release`: the test releases the write only after the read has returned.
    let (write_open_tx, write_open_rx) = oneshot::channel::<()>();
    let (release_tx, release_rx) = oneshot::channel::<()>();

    let writer = Arc::clone(&store);
    let write = tokio::spawn(async move {
        writer
            .write_transaction_async(move |tx| {
                // Real write work inside the txn.
                tx.execute_batch("CREATE TEMP TABLE m23_probe (n INTEGER);")
                    .map_err(|err| StoreError::Failure(err.to_string()))?;
                // Announce the txn is open, then block (on the blocking pool)
                // until the read has completed and the test releases us.
                let _ = write_open_tx.send(());
                let _ = release_rx.blocking_recv();
                Ok(())
            })
            .await
    });

    // Wait — without blocking an async worker — until the write txn is open.
    write_open_rx
        .await
        .expect("write closure should signal that its txn is open");

    // With the write still in progress, a concurrent read must complete promptly.
    let reader = Arc::clone(&store);
    let count = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_async(|conn| {
            conn.query_row("SELECT COUNT(*) FROM message", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|err| StoreError::Failure(err.to_string()))
        }),
    )
    .await
    .expect("read must complete while the write txn is still open")
    .expect("read query should succeed");
    assert_eq!(
        count, 1,
        "the committed message must be visible to the concurrent read"
    );

    // Release the write so it can commit, then confirm it committed cleanly.
    let _ = release_tx.send(());
    write
        .await
        .expect("write task should not panic")
        .expect("write should commit after release");
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N16 / M27 sub-unit (c))
//
// `read_connection` is a plain blocking function (see `store.rs`), reachable
// with no tokio runtime at all — so this is a genuine `#[test]`, not
// `#[tokio::test]`, driving real OS threads to prove the bound holds under
// true concurrent contention rather than cooperative task scheduling.
#[test]
fn read_connection_pool_bounds_peak_concurrency() {
    use crate::store::MAX_READ_CONNECTIONS;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let root = crate::test_support::temp_root();
    let store = Arc::new(DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap());

    // 3x the cap: enough concurrent contenders that, without the semaphore,
    // every thread would open its own fresh SQLite connection at once (N16's
    // failure mode — N concurrent readers, N uncapped connections).
    let thread_count = MAX_READ_CONNECTIONS * 3;
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let store = Arc::clone(&store);
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            std::thread::spawn(move || {
                // Blocks (queues) rather than opening an unbounded new
                // connection once `MAX_READ_CONNECTIONS` are already open.
                let _connection = store.read_connection().expect("read connection");
                let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now_active, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(30));
                active.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("reader thread should not panic");
    }

    let observed_peak = peak.load(Ordering::SeqCst);
    assert!(
        observed_peak <= MAX_READ_CONNECTIONS,
        "peak concurrent read connections ({observed_peak}) must not exceed the cap ({MAX_READ_CONNECTIONS})"
    );
}
