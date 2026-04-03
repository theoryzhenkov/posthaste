use super::*;
use std::time::Duration;

/// Creates all tables and indexes if they do not exist. Tables use
/// `(account_id, ...)` composite keys to enforce the account-scoping invariant.
///
/// @spec spec/L1-sync#sqlite-schema
/// @spec spec/L0-accounts#the-invariant
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

            CREATE INDEX IF NOT EXISTS idx_message_thread
                ON message (account_id, thread_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_conversation
                ON message (conversation_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_rfc_message_id
                ON message (rfc_message_id);
            CREATE INDEX IF NOT EXISTS idx_message_mailbox
                ON message_mailbox (account_id, mailbox_id);
            CREATE INDEX IF NOT EXISTS idx_message_keyword
                ON message_keyword (account_id, keyword);
            CREATE INDEX IF NOT EXISTS idx_event_log_lookup
                ON event_log (account_id, topic, mailbox_id, seq);
            CREATE INDEX IF NOT EXISTS idx_conversation_message_lookup
                ON conversation_message (account_id, message_id);
            ",
        )
        .map_err(sql_to_store_error)?;
    Ok(())
}

/// Configures WAL journal mode and a 5-second busy timeout.
pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
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
