mod sql;

use super::columns::ensure_column;
use super::*;

/// Creates all tables and indexes if they do not exist. Tables use
/// `(account_id, ...)` composite keys to enforce the account-scoping invariant.
///
/// @spec docs/L1-sync#sqlite-schema
/// @spec docs/L0-accounts#the-invariant
pub(crate) fn init_schema(connection: &mut Connection) -> Result<(), StoreError> {
    migrate_legacy_message_fts(connection)?;
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
    // B4: resumable partial-initial-sync checkpoint for interrupted first syncs.
    ensure_column(
        connection,
        "imap_mailbox_sync_state",
        "partial_initial_uid",
        "ALTER TABLE imap_mailbox_sync_state ADD COLUMN partial_initial_uid INTEGER",
    )?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_cache_rescore_priority
             ON cache_rescore_queue (account_id, rescore_priority DESC, queued_at, message_id)",
            [],
        )
        .map_err(sql_to_store_error)?;
    // The body-cache-object repair (three correlated `NOT EXISTS` full-table
    // scans against `message`) used to run right here, unconditionally, on
    // every open — blocking `DatabaseStore::open`'s return (and therefore
    // every first read/write) behind an unbounded startup scan (N15 / M27
    // sub-unit (b)). It is no longer called from schema init: the
    // composition root now runs [`crate::store::DatabaseStore::repair_body_cache_objects`]
    // as a deferred post-startup task instead, off this path and its
    // (pre-`write_connection`-`Mutex`) init-time lock.
    Ok(())
}

/// One-time migration for the `message_fts` body-indexing change: the
/// prototype index was external-content over `message` directly (header
/// columns only, no `body`). The current definition is external-content over
/// the `message_fts_content` view (headers + the body-cache's `body_text`),
/// with an extended trigger set. FTS5 tables cannot be `ALTER`ed into a new
/// column/content shape, so an old-shape table (recognised by its
/// `sqlite_master` SQL not naming the content view) is dropped here together
/// with its triggers; `SCHEMA_SQL`'s `IF NOT EXISTS` block then recreates the
/// new shape empty.
///
/// Repopulation is deliberately NOT done here: it is an unbounded scan of all
/// messages + cached bodies, and this function runs inside `DatabaseStore::open`.
/// The composition root runs [`crate::store::DatabaseStore::backfill_message_fts`]
/// as a deferred post-startup task (the address-book-backfill pattern), which
/// detects the empty-index-with-messages state this migration leaves behind
/// and issues the FTS5 `rebuild`. Until that completes, text search on an
/// upgraded database is degraded (one time, per upgrade).
fn migrate_legacy_message_fts(connection: &Connection) -> Result<(), StoreError> {
    let existing_sql: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'message_fts'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_to_store_error)?;
    let Some(existing_sql) = existing_sql else {
        return Ok(()); // Fresh database: nothing to migrate.
    };
    if existing_sql.contains("message_fts_content") {
        return Ok(()); // Already the body-indexing shape.
    }
    connection
        .execute_batch(
            "DROP TRIGGER IF EXISTS message_fts_ai;
             DROP TRIGGER IF EXISTS message_fts_ad;
             DROP TRIGGER IF EXISTS message_fts_au;
             DROP TRIGGER IF EXISTS message_body_fts_ai;
             DROP TRIGGER IF EXISTS message_body_fts_au;
             DROP TRIGGER IF EXISTS message_body_fts_ad;
             DROP TABLE message_fts;",
        )
        .map_err(sql_to_store_error)?;
    Ok(())
}
