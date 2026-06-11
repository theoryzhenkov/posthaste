use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-store-cache-test-{now}-{seq}"))
}

fn candidate(message_id: &str, priority: f64, fetch_bytes: u64) -> CacheCandidate {
    CacheCandidate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        value_bytes: fetch_bytes,
        fetch_bytes,
        priority,
        reason: "test".to_string(),
    }
}

fn insert_message_metadata(
    store: &DatabaseStore,
    message_id: &str,
    received_at: &str,
) -> Result<(), StoreError> {
    store.write_transaction(|tx| {
        tx.execute(
            "INSERT OR IGNORE INTO mailbox (account_id, id, name, role)
             VALUES ('primary', 'inbox', 'Inbox', 'inbox')",
            [],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "INSERT INTO message (
                account_id, id, thread_id, received_at, size, is_read, is_flagged
             ) VALUES ('primary', ?1, 'thread-1', ?2, 4096, 0, 1)",
            params![message_id, received_at],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "INSERT INTO message_mailbox (account_id, message_id, mailbox_id)
             VALUES ('primary', ?1, 'inbox')",
            params![message_id],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })
}

fn set_last_scored_at(
    store: &DatabaseStore,
    message_id: &str,
    last_scored_at: &str,
) -> Result<(), StoreError> {
    store.write_transaction(|tx| {
        tx.execute(
            "UPDATE cache_object
             SET last_scored_at = ?2
             WHERE account_id = 'primary' AND message_id = ?1",
            params![message_id, last_scored_at],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })
}

mod ledger;
mod priority_materialization;
mod signals;
mod stale_rescore;
