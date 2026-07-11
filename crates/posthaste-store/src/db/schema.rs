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
    // Parsed List-Unsubscribe targets (RFC 2369/8058) as `ListUnsubscribe`
    // JSON; NULL = no valid target known.
    ensure_column(
        connection,
        "message",
        "list_unsubscribe",
        "ALTER TABLE message ADD COLUMN list_unsubscribe TEXT",
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
    // Scheduled sends (undo-send / send-later): the earliest flush time for a
    // held send op, normalized UTC whole-second RFC 3339 (NULL = flush now).
    ensure_column(
        connection,
        "outbox_operation",
        "send_at",
        "ALTER TABLE outbox_operation ADD COLUMN send_at TEXT",
    )?;
    // D152: undo-send hold deadline on the daemon's monotonic-anchored clock
    // (send-later keeps `send_at`; the two are judged on their own clocks).
    ensure_column(
        connection,
        "outbox_operation",
        "hold_until_mono",
        "ALTER TABLE outbox_operation ADD COLUMN hold_until_mono INTEGER",
    )?;
    // D155: the payload envelope version — existing rows are the v1 shapes.
    ensure_column(
        connection,
        "outbox_operation",
        "payload_version",
        "ALTER TABLE outbox_operation ADD COLUMN payload_version INTEGER NOT NULL DEFAULT 1",
    )?;
    connection
        .execute(
            // Partial index for the scheduler tick's "any send due?" probe and
            // the flush filter; only scheduled rows (a tiny minority) appear.
            "CREATE INDEX IF NOT EXISTS idx_outbox_send_at
             ON outbox_operation (account_id, send_at) WHERE send_at IS NOT NULL",
            [],
        )
        .map_err(sql_to_store_error)?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_outbox_hold_until_mono
             ON outbox_operation (account_id, hold_until_mono) WHERE hold_until_mono IS NOT NULL",
            [],
        )
        .map_err(sql_to_store_error)?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_cache_rescore_priority
             ON cache_rescore_queue (account_id, rescore_priority DESC, queued_at, message_id)",
            [],
        )
        .map_err(sql_to_store_error)?;
    // Strictly after the ensure_column evolution above: these views reference
    // late-added `message` columns, and CREATE VIEW validates its SELECT (see
    // the constant's doc in sql.rs).
    connection
        .execute_batch(sql::EFFECTIVE_VIEWS_SQL)
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

/// The store's schema version (M84 / NS2 Slice 0), stamped into SQLite's
/// `PRAGMA user_version`. Policy: ADDITIVE evolution (new tables/columns/
/// views/indexes) stays in the idempotent [`init_schema`] path; DESTRUCTIVE
/// or TRANSFORMATIVE changes (drops, renames, data rewrites, trigger
/// replacements) are numbered migrations below — run exactly once per
/// database, each in its own transaction, in order.
pub(crate) const SCHEMA_VERSION: i64 = 2;

/// The full open-time schema flow (replaces bare `init_schema` at the open
/// call site):
///
/// - FRESH database (no `message` table): create the current shape and stamp
///   [`SCHEMA_VERSION`] — no migrations to run.
/// - NEWER database (`user_version` above ours): refuse with
///   [`StoreError::Conflict`] — deliberately NOT `Corruption`, so the repair
///   path never quarantines a database written by a newer build.
/// - OLDER database: run each pending migration in its own transaction,
///   stamping `user_version` atomically with it, then run the idempotent
///   additive evolution.
pub(crate) fn prepare_schema(connection: &mut Connection) -> Result<(), StoreError> {
    let fresh: bool = connection
        .query_row(
            "SELECT NOT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'message'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    if fresh {
        init_schema(connection)?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(sql_to_store_error)?;
        return Ok(());
    }

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql_to_store_error)?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Conflict(format!(
            "database schema version {version} is newer than this build supports \
             ({SCHEMA_VERSION}); refusing to open (downgrade guard)"
        )));
    }
    for next in (version + 1)..=SCHEMA_VERSION {
        let tx = connection.transaction().map_err(sql_to_store_error)?;
        apply_migration(&tx, next)?;
        tx.pragma_update(None, "user_version", next)
            .map_err(sql_to_store_error)?;
        tx.commit().map_err(sql_to_store_error)?;
    }
    init_schema(connection)
}

fn apply_migration(tx: &Connection, version: i64) -> Result<(), StoreError> {
    match version {
        1 => v1_retire_mailbox_counters(tx),
        2 => v2_recover_conflicted_outbox_rows(tx),
        other => Err(StoreError::Failure(format!(
            "unknown schema migration {other}"
        ))),
    }
}

/// v1 (NS1 wave 3 → M84): the incremental mailbox-counter machinery is
/// retired — counts are a live derivation over the `_effective` views
/// (read/mailbox.rs). Drops the maintenance triggers (previously DROPped
/// unconditionally on every open) and the dead counter columns. Trigger drops
/// MUST precede the column drops (SQLite refuses to drop a column a trigger
/// references).
fn v1_retire_mailbox_counters(tx: &Connection) -> Result<(), StoreError> {
    tx.execute_batch(
        "DROP TRIGGER IF EXISTS mailbox_counters_message_mailbox_ai;
         DROP TRIGGER IF EXISTS mailbox_counters_message_mailbox_ad;
         DROP TRIGGER IF EXISTS mailbox_counters_message_read_au;",
    )
    .map_err(sql_to_store_error)?;
    for column in ["unread_emails", "total_emails"] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM pragma_table_info('mailbox') WHERE name = ?1
                 )",
                [column],
                |row| row.get(0),
            )
            .map_err(sql_to_store_error)?;
        if exists {
            tx.execute_batch(&format!("ALTER TABLE mailbox DROP COLUMN {column}"))
                .map_err(sql_to_store_error)?;
        }
    }
    Ok(())
}

/// v2 (D155): the first-outbox-design legacy state `"conflicted"` is rewritten
/// to `"pending"` ONCE, replacing the silent read-time fudge the state parser
/// carried ("conflicted" => Pending) — the parser is now strict, so an unknown
/// state is an error instead of a guess.
fn v2_recover_conflicted_outbox_rows(tx: &Connection) -> Result<(), StoreError> {
    // Guard: the table may not exist on very old fixtures; IF-EXISTS via probe.
    let has_table: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'outbox_operation')",
            [],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    if has_table {
        tx.execute(
            "UPDATE outbox_operation SET state = 'pending' WHERE state = 'conflicted'",
            [],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}
