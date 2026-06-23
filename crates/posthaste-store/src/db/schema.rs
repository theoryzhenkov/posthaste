mod sql;

use super::columns::ensure_column;
use super::*;

/// Creates all tables and indexes if they do not exist. Tables use
/// `(account_id, ...)` composite keys to enforce the account-scoping invariant.
///
/// @spec docs/L1-sync#sqlite-schema
/// @spec docs/L0-accounts#the-invariant
pub(crate) fn init_schema(connection: &mut Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(sql::SCHEMA_SQL)
        .map_err(sql_to_store_error)?;
    ensure_column(
        connection,
        "message",
        "to_json",
        "ALTER TABLE message ADD COLUMN to_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "message",
        "draft_id",
        "ALTER TABLE message ADD COLUMN draft_id TEXT",
    )?;
    ensure_column(
        connection,
        "cache_rescore_queue",
        "rescore_priority",
        "ALTER TABLE cache_rescore_queue ADD COLUMN rescore_priority REAL NOT NULL DEFAULT 0",
    )?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_cache_rescore_priority
             ON cache_rescore_queue (account_id, rescore_priority DESC, queued_at, message_id)",
            [],
        )
        .map_err(sql_to_store_error)?;
    crate::cache::repair_missing_body_cache_objects(connection)?;
    Ok(())
}
