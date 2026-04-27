use super::*;
use std::time::Duration;

/// Creates all tables and indexes if they do not exist. Tables use
/// `(account_id, ...)` composite keys to enforce the account-scoping invariant.
///
/// @spec docs/L1-sync#sqlite-schema
/// @spec docs/L0-accounts#the-invariant
pub(crate) fn init_schema(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mailbox (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                role TEXT,
                unread_emails INTEGER NOT NULL DEFAULT 0,
                total_emails INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS message (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                conversation_id TEXT,
                remote_blob_id TEXT,
                subject TEXT,
                normalized_subject TEXT,
                from_name TEXT,
                from_email TEXT,
                preview TEXT,
                received_at TEXT NOT NULL,
                has_attachment INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                is_read INTEGER NOT NULL DEFAULT 1,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                rfc_message_id TEXT,
                in_reply_to TEXT,
                references_json TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS conversation (
                id TEXT PRIMARY KEY,
                subject TEXT,
                normalized_subject TEXT,
                latest_received_at TEXT NOT NULL,
                latest_source_id TEXT NOT NULL,
                latest_message_id TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                unread_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS conversation_message (
                conversation_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                PRIMARY KEY (conversation_id, account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS message_mailbox (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS message_keyword (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, keyword)
            );

            CREATE TABLE IF NOT EXISTS message_body (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                body_html TEXT,
                body_text TEXT,
                raw_path TEXT,
                raw_sha256 TEXT,
                raw_size INTEGER,
                raw_mime_type TEXT,
                fetched_at TEXT,
                PRIMARY KEY (account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS message_attachment (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                id TEXT NOT NULL,
                blob_id TEXT NOT NULL,
                part_id TEXT,
                filename TEXT,
                mime_type TEXT NOT NULL,
                size INTEGER NOT NULL DEFAULT 0,
                disposition TEXT,
                cid TEXT,
                is_inline INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, message_id, id)
            );

            CREATE TABLE IF NOT EXISTS thread_view (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                email_ids TEXT NOT NULL,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS sync_cursor (
                account_id TEXT NOT NULL,
                object_type TEXT NOT NULL,
                state TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, object_type)
            );

            CREATE TABLE IF NOT EXISTS imap_mailbox_sync_state (
                account_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                mailbox_name TEXT NOT NULL,
                uid_validity INTEGER NOT NULL,
                highest_uid INTEGER,
                highest_modseq TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS imap_message_location (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                uid_validity INTEGER NOT NULL,
                uid INTEGER NOT NULL,
                modseq TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS event_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                mailbox_id TEXT,
                message_id TEXT,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS source_projection (
                source_id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS automation_backfill_job (
                account_id TEXT NOT NULL,
                rule_fingerprint TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                queued_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, rule_fingerprint)
            );

            CREATE TABLE IF NOT EXISTS sender_address_cache (
                account_id TEXT NOT NULL,
                normalized_email TEXT NOT NULL,
                email TEXT NOT NULL,
                name TEXT,
                last_used_at TEXT NOT NULL,
                PRIMARY KEY (account_id, normalized_email)
            );

            CREATE TABLE IF NOT EXISTS cache_object (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                layer TEXT NOT NULL,
                object_id TEXT NOT NULL DEFAULT '',
                fetch_unit TEXT NOT NULL,
                state TEXT NOT NULL,
                value_bytes INTEGER NOT NULL DEFAULT 0,
                fetch_bytes INTEGER NOT NULL DEFAULT 0,
                priority REAL NOT NULL DEFAULT 0,
                reason TEXT NOT NULL DEFAULT '',
                last_scored_at TEXT NOT NULL,
                last_accessed_at TEXT,
                fetched_at TEXT,
                error_code TEXT,
                PRIMARY KEY (account_id, message_id, layer, object_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS cache_message_signal (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                search_total_messages INTEGER,
                search_result_count INTEGER,
                search_result_rank INTEGER,
                search_seen_count INTEGER NOT NULL DEFAULT 0,
                last_search_seen_at TEXT,
                thread_activity_score REAL,
                sender_affinity_score REAL,
                local_behavior_score REAL,
                direct_user_boost REAL,
                pinned INTEGER,
                dirty_at TEXT,
                PRIMARY KEY (account_id, message_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS cache_rescore_queue (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                queued_at TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_message_thread
                ON message (account_id, thread_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_account_received
                ON message (account_id, received_at, id);
            CREATE INDEX IF NOT EXISTS idx_message_account_from_sort
                ON message (account_id, LOWER(COALESCE(from_name, from_email, '')), id);
            CREATE INDEX IF NOT EXISTS idx_message_account_subject_sort
                ON message (account_id, LOWER(COALESCE(subject, '')), id);
            CREATE INDEX IF NOT EXISTS idx_message_account_flagged_sort
                ON message (account_id, is_flagged, id);
            CREATE INDEX IF NOT EXISTS idx_message_account_attachment_sort
                ON message (account_id, has_attachment, id);
            CREATE INDEX IF NOT EXISTS idx_message_conversation
                ON message (conversation_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_rfc_message_id
                ON message (rfc_message_id);
            CREATE INDEX IF NOT EXISTS idx_message_mailbox
                ON message_mailbox (account_id, mailbox_id);
            CREATE INDEX IF NOT EXISTS idx_message_keyword
                ON message_keyword (account_id, keyword);
            CREATE INDEX IF NOT EXISTS idx_message_attachment_blob
                ON message_attachment (account_id, blob_id);
            CREATE INDEX IF NOT EXISTS idx_event_log_lookup
                ON event_log (account_id, topic, mailbox_id, seq);
            CREATE INDEX IF NOT EXISTS idx_conversation_message_lookup
                ON conversation_message (account_id, message_id);
            CREATE INDEX IF NOT EXISTS idx_automation_backfill_pending
                ON automation_backfill_job (account_id, status, updated_at);
            CREATE INDEX IF NOT EXISTS idx_sender_address_cache_recent
                ON sender_address_cache (last_used_at DESC, account_id);
            CREATE INDEX IF NOT EXISTS idx_cache_fetch_candidates
                ON cache_object (account_id, state, layer, priority DESC);
            CREATE INDEX IF NOT EXISTS idx_cache_cached_bytes
                ON cache_object (state, fetch_bytes);
            CREATE INDEX IF NOT EXISTS idx_cache_signal_dirty
                ON cache_message_signal (account_id, dirty_at);
            CREATE INDEX IF NOT EXISTS idx_cache_rescore_queue
                ON cache_rescore_queue (account_id, queued_at);
            ",
        )
        .map_err(sql_to_store_error)?;
    crate::cache::repair_missing_body_cache_objects(connection)?;
    Ok(())
}

/// Configures WAL journal mode, foreign-key enforcement, and a 5-second busy timeout.
pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "journal_mode", "wal")
        .map_err(sql_to_store_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(sql_to_store_error)?;
    Ok(())
}

/// Returns the current time as an ISO 8601 string.
pub(crate) fn now_iso8601() -> Result<String, StoreError> {
    domain_now_iso8601().map_err(StoreError::Failure)
}

/// Parses a `sync_cursor.object_type` string into a `SyncObject` enum.
pub(crate) fn parse_sync_object(value: &str) -> Result<SyncObject, rusqlite::Error> {
    match value {
        "mailbox" => Ok(SyncObject::Mailbox),
        "message" => Ok(SyncObject::Message),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown sync object {other}"),
            )),
        )),
    }
}

/// Converts a bool to SQLite integer (0/1).
pub(crate) fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

/// Wraps a rusqlite error into `StoreError::Failure`.
pub(crate) fn sql_to_store_error(err: rusqlite::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

/// Wraps an I/O error into `StoreError::Failure`.
pub(crate) fn io_to_store_error(err: std::io::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

/// Wraps a JSON serialization error into `StoreError::Failure`.
pub(crate) fn json_to_store_error(err: impl std::error::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}
