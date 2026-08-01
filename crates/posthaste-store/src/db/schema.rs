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
    // The recipient fields the message-field registry projects beside `to_json`.
    // Empty-array default rather than NULL: "no cc/bcc/reply-to" is the normal
    // state, not missing data (a delivered message's Bcc is stripped in
    // transit), and the registry renders empty as a non-render either way.
    // Both planes, so the `message_effective` UNION keeps matching column lists.
    for table in ["message", "message_overlay"] {
        for column in ["cc_json", "bcc_json", "reply_to_json"] {
            ensure_column(
                connection,
                table,
                column,
                &format!("ALTER TABLE {table} ADD COLUMN {column} TEXT NOT NULL DEFAULT '[]'"),
            )?;
        }
    }
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
    // Causal-truncation markers for settled ('applied') ops: when the op
    // settled on the daemon's monotonic-anchored clock, and the provider sync
    // position (stored-cursor encoding) that includes its change. NULL
    // `settled_at_mono` on an 'applied' row (a legacy row) is
    // truncate-eligible on any completed sync cycle; NULL `settled_watermark`
    // means no usable position — the cycle rule alone truncates.
    ensure_column(
        connection,
        "outbox_operation",
        "settled_at_mono",
        "ALTER TABLE outbox_operation ADD COLUMN settled_at_mono INTEGER",
    )?;
    ensure_column(
        connection,
        "outbox_operation",
        "settled_watermark",
        "ALTER TABLE outbox_operation ADD COLUMN settled_watermark TEXT",
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
pub(crate) const SCHEMA_VERSION: i64 = 6;

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
        3 => v3_drop_outbox_depends_on(tx),
        4 => v4_normalize_received_at_to_utc(tx),
        5 => v5_reconcile_pre_slice3_debris(tx),
        6 => v6_refresh_message_effective_view(tx),
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

/// v3 (D174 / NS2 Slice 3): cross-operation dependency chains are deleted —
/// state assertions and draft saves both coalesce, and everything else relies
/// on the flusher's insertion-order drain — so the `depends_on` column goes.
/// In-queue chain data needs no rewrite: a pending dependent simply flushes in
/// insertion order (after what used to be its dependency).
fn v3_drop_outbox_depends_on(tx: &Connection) -> Result<(), StoreError> {
    let has_column: bool = tx
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('outbox_operation') WHERE name = 'depends_on'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    if has_column {
        tx.execute_batch("ALTER TABLE outbox_operation DROP COLUMN depends_on")
            .map_err(sql_to_store_error)?;
    }
    Ok(())
}

/// v4: rewrite offset-bearing `received_at` values to the canonical UTC `…Z`
/// RFC 3339 shape. The IMAP header mapper used to serialize the Date header
/// with its original offset (`2026-07-17T13:23:00+02:00`) while the JMAP path
/// always emitted UTC; the store sorts these columns as TEXT, so mixed
/// offsets made lexicographic order diverge from chronological order — a
/// row could land pages away from its true position (the mail list's
/// out-of-order DATE RECEIVED bug). The ingestion fix normalizes new rows;
/// this rewrites the ones already stored, across every column that carries
/// the value (`message`, the optimistic `message_overlay`, and the
/// `conversation` projection's `latest_received_at`).
///
/// `strftime` parses ISO-8601 with a trailing offset and renders UTC; a
/// value it cannot parse yields NULL and is left untouched (COALESCE) —
/// better an odd sort key than destroying data or violating NOT NULL.
fn v4_normalize_received_at_to_utc(tx: &Connection) -> Result<(), StoreError> {
    for (table, column) in [
        ("message", "received_at"),
        ("message_overlay", "received_at"),
        ("conversation", "latest_received_at"),
    ] {
        let has_table: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(sql_to_store_error)?;
        if !has_table {
            continue;
        }
        tx.execute(
            &format!(
                "UPDATE {table}
                 SET {column} = COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', {column}), {column})
                 WHERE {column} NOT LIKE '%Z'"
            ),
            [],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

/// v5: reconcile a pre-replay-model database — the drafts and sends the old
/// pin machinery stranded in the overlay plane before content ops existed. A
/// pin here is a non-tombstone `message_overlay` row that carries locally
/// authored identity: a draft (`draft_id` set) or a provisional Sent
/// (`rfc_message_id LIKE 'phsend-%'`). Tombstone rows are NOT pins — they are
/// pending Destroys the replay owns — and are left alone.
///
/// The OVERRIDING RULE is preservation over deletion: this runs against real
/// authored words, and a wrong delete is unrecoverable. A pin is deleted ONLY
/// when its content is provably reconstructible from base or the live log;
/// anything ambiguous is PARKED as a visible, discardable content op, never
/// silently dropped. Three exhaustive cases, checked in order:
///
/// - DROP: a LIVE owning op resolves to the pin. Replay re-derives the row from
///   that op after upgrade, so the stale overlay copy is deleted.
/// - DELETE: the pin is PROVABLY DERIVED debris — a base `message` row with the
///   pin's exact id AND identical content (a pure override base now serves, not
///   a divergent edit), or a `phsend-` pin whose provider Sent copy has already
///   arrived in base (matched by the shared `Message-ID` token). A same-id base
///   row whose content DIFFERS is NOT derived — the overlay shadows it, so the
///   edit survives nowhere else — and falls through to PARK.
/// - PARK: everything else — content that exists nowhere else. Synthesize a
///   `Failed` `DraftCreate` content op from the pin's surviving fields (subject,
///   recipients, threading; the body was only ever in the lost op payload, so
///   an empty body is the honest floor) and self-map its draft alias, then
///   delete the overlay copy. `is_replayable` keeps a failed content op
///   folding, so the row reappears in Drafts and the outbox for the user to
///   keep or discard.
///
/// Deterministic and idempotent-safe: pure SQL plus serde over already-stored
/// bytes; the synthesized op's id derives from the pin id and its timestamps
/// from the pin's own `received_at` (no clock, no randomness), with
/// `INSERT OR IGNORE`, so a re-run mints no duplicates. A fresh/clean database
/// finds no pins and is a clean no-op.
/// One stranded overlay pin salvaged from the migration's collection pass.
struct Pin {
    account_id: String,
    id: String,
    subject: Option<String>,
    from_name: Option<String>,
    from_email: Option<String>,
    received_at: String,
    to_json: String,
    in_reply_to: Option<String>,
    references_json: String,
    rfc_message_id: Option<String>,
}

fn has_outbox_operation(tx: &Connection) -> Result<bool, StoreError> {
    tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'outbox_operation'
         )",
        [],
        |row| row.get(0),
    )
    .map_err(sql_to_store_error)
}

fn v5_reconcile_pre_slice3_debris(tx: &Connection) -> Result<(), StoreError> {
    // Guard: the overlay plane may be absent on very old fixtures. No overlay,
    // no pins — clean no-op (idempotent-safe).
    let has_overlay: bool = tx
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'message_overlay'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    if !has_overlay {
        return Ok(());
    }

    // Migrations run before the additive `init_schema` pass, so on a database
    // predating these `outbox_operation` columns they are not yet present. The
    // PARK insert names all three; ensure them here (the same idempotent
    // `ensure_column` `init_schema` uses) so a genuinely-old database can be
    // reconciled. On a database that already has them this is a no-op probe.
    if has_outbox_operation(tx)? {
        ensure_column(
            tx,
            "outbox_operation",
            "send_at",
            "ALTER TABLE outbox_operation ADD COLUMN send_at TEXT",
        )?;
        ensure_column(
            tx,
            "outbox_operation",
            "hold_until_mono",
            "ALTER TABLE outbox_operation ADD COLUMN hold_until_mono INTEGER",
        )?;
        ensure_column(
            tx,
            "outbox_operation",
            "payload_version",
            "ALTER TABLE outbox_operation ADD COLUMN payload_version INTEGER NOT NULL DEFAULT 1",
        )?;
    }

    // Collect every debris pin first (drop the statement before mutating).
    let pins: Vec<Pin> = {
        let mut statement = tx
            .prepare(
                "SELECT account_id, id, subject, from_name, from_email, received_at,
                        to_json, in_reply_to, references_json, rfc_message_id
                 FROM message_overlay
                 WHERE tombstone = 0
                   AND (draft_id IS NOT NULL OR rfc_message_id LIKE 'phsend-%')",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(Pin {
                    account_id: row.get(0)?,
                    id: row.get(1)?,
                    subject: row.get(2)?,
                    from_name: row.get(3)?,
                    from_email: row.get(4)?,
                    received_at: row.get(5)?,
                    to_json: row.get(6)?,
                    in_reply_to: row.get(7)?,
                    references_json: row.get(8)?,
                    rfc_message_id: row.get(9)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let mut pins = Vec::new();
        for pin in rows {
            pins.push(pin.map_err(sql_to_store_error)?);
        }
        pins
    };

    for pin in &pins {
        if pin_has_live_owning_op(tx, pin.account_id.as_str(), pin.id.as_str())? {
            // DROP: replay re-derives the row from the live op.
            delete_overlay_pin(tx, pin.account_id.as_str(), pin.id.as_str())?;
        } else if pin_is_provably_derived(
            tx,
            pin.account_id.as_str(),
            pin.id.as_str(),
            pin.rfc_message_id.as_deref(),
        )? {
            // DELETE: base reconstructs the row.
            delete_overlay_pin(tx, pin.account_id.as_str(), pin.id.as_str())?;
        } else {
            // PARK: content that exists nowhere else — recover it, then delete
            // the stale overlay copy (replay re-materializes it from the op).
            park_pin_as_content_op(tx, pin)?;
            delete_overlay_pin(tx, pin.account_id.as_str(), pin.id.as_str())?;
        }
    }
    Ok(())
}

/// v6: republish `message_effective` so it projects the newly added
/// `cc_json`/`bcc_json`/`reply_to_json`.
///
/// The columns themselves are additive and land via `ensure_column`, but the
/// VIEW over them is not self-healing: `init_schema` creates it with
/// `CREATE VIEW IF NOT EXISTS`, so an existing database keeps whatever column
/// list it was created with and every read through the view would keep seeing
/// the old shape. A view holds no data, so refreshing it is just a DROP —
/// `init_schema` recreates it from `EFFECTIVE_VIEWS_SQL` immediately after the
/// migration loop, and strictly after the `ensure_column` calls the new SELECT
/// depends on.
///
/// This is the numbered-migration case for any future change to a view's
/// shape, not just this one.
fn v6_refresh_message_effective_view(tx: &Connection) -> Result<(), StoreError> {
    tx.execute_batch("DROP VIEW IF EXISTS message_effective")
        .map_err(sql_to_store_error)
}

/// DROP predicate: a still-live op in the log resolves to this pin's row id —
/// directly (`entity_id` == the pin id: a send whose provisional row id is the
/// op id, or a self-mapped draft whose key is the pin id) or through a
/// `draft_alias` mapping the op's stable key to the pin id (a rotated draft). A
/// failed op only counts when it is a content op (a failed intent folds
/// nothing and would not re-derive the row).
fn pin_has_live_owning_op(
    tx: &Connection,
    account_id: &str,
    pin_id: &str,
) -> Result<bool, StoreError> {
    tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM outbox_operation o
             WHERE o.account_id = ?1
               AND (o.state != 'failed'
                    OR o.kind IN ('draftCreate', 'draftUpdate', 'send'))
               AND (
                   o.entity_id = ?2
                   OR EXISTS(
                       SELECT 1 FROM draft_alias a
                       WHERE a.account_id = ?1
                         AND a.entity_id = ?2
                         AND a.draft_key = o.entity_id
                   )
               )
         )",
        params![account_id, pin_id],
        |row| row.get(0),
    )
    .map_err(sql_to_store_error)
}

/// DELETE predicate: the pin is provably reconstructible from base. Two checks,
/// each naming the base row that serves it. The overriding rule is preservation
/// over deletion, so a same-id base row deletes the overlay ONLY when its
/// content is byte-for-byte the base's — a divergent override carries the user's
/// unflushed edit and MUST fall through to PARK:
///
/// - a base `message` row with the pin's exact id AND identical content on every
///   authored/derived column (the overlay was a pure override base now serves,
///   not an edit stranded nowhere else); or
/// - a `phsend-` pin whose provider Sent copy has arrived in base, matched by
///   the shared `Message-ID` token (the pin's `rfc_message_id` up to and
///   including the `@` — [`send_identity_prefix`]'s shape).
///
/// A same-id base row whose content DIFFERS is deliberately NOT provably derived:
/// the overlay shadows it in `message_effective`, so the divergent columns are
/// the only surviving copy of the user's edit. Returning `false` sends it to
/// PARK, where its words come back as a visible, discardable content op.
fn pin_is_provably_derived(
    tx: &Connection,
    account_id: &str,
    pin_id: &str,
    rfc_message_id: Option<&str>,
) -> Result<bool, StoreError> {
    // A same-id base row proves derivation only when the overlay adds NOTHING
    // over base — every authored/derived content column matches (null-safe
    // `IS`). Any divergence means the overlay carries an edit that exists
    // nowhere else; it is not derivable and must be preserved. `draft_id` is
    // excluded: it is the pin marker (always set on the overlay), and a synced
    // base copy legitimately lacks it, so it is not an authored-content
    // divergence.
    let base_covers_content: bool = tx
        .query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM message m
                 JOIN message_overlay o
                   ON o.account_id = m.account_id AND o.id = m.id
                 WHERE m.account_id = ?1
                   AND m.id = ?2
                   AND m.subject            IS o.subject
                   AND m.normalized_subject IS o.normalized_subject
                   AND m.from_name          IS o.from_name
                   AND m.from_email         IS o.from_email
                   AND m.to_json            IS o.to_json
                   AND m.preview            IS o.preview
                   AND m.received_at        IS o.received_at
                   AND m.has_attachment     IS o.has_attachment
                   AND m.size               IS o.size
                   AND m.is_read            IS o.is_read
                   AND m.is_flagged         IS o.is_flagged
                   AND m.rfc_message_id     IS o.rfc_message_id
                   AND m.in_reply_to        IS o.in_reply_to
                   AND m.references_json    IS o.references_json
             )",
            params![account_id, pin_id],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    if base_covers_content {
        return Ok(true);
    }
    // phsend adoption: only when the pin's Message-ID carries the send token
    // (`phsend-<op>@…`); take the prefix up to and including the first `@`.
    if let Some(rfc) = rfc_message_id {
        if let Some(at) = rfc.find('@') {
            let token = &rfc[..=at];
            if token.starts_with("phsend-") {
                let escaped = token
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let adopted: bool = tx
                    .query_row(
                        "SELECT EXISTS(
                             SELECT 1 FROM message m
                             WHERE m.account_id = ?1
                               AND m.rfc_message_id LIKE ?2 || '%' ESCAPE '\\'
                         )",
                        params![account_id, escaped],
                        |row| row.get(0),
                    )
                    .map_err(sql_to_store_error)?;
                return Ok(adopted);
            }
        }
    }
    Ok(false)
}

/// PARK: synthesize a `Failed` `DraftCreate` content op from the pin's
/// surviving fields so replay materializes a visible, editable draft the user
/// can keep or discard. The op is fully deterministic — its id derives from the
/// (account-scoped, unique) pin id and every timestamp is the pin's own
/// `received_at` — so `INSERT OR IGNORE` makes a re-run a clean no-op. The body
/// was only ever in the now-gone op payload; an empty body is the honest floor
/// (subject/recipients/threading come back).
fn park_pin_as_content_op(tx: &Connection, pin: &Pin) -> Result<(), StoreError> {
    let from = pin.from_email.as_ref().map(|email| Recipient {
        name: pin.from_name.clone(),
        email: email.clone(),
    });
    let to: Vec<Recipient> = serde_json::from_str(&pin.to_json).unwrap_or_default();
    let references = {
        let refs: Vec<String> = serde_json::from_str(&pin.references_json).unwrap_or_default();
        (!refs.is_empty()).then(|| refs.join(" "))
    };
    let request = posthaste_domain_model::SendMessageRequest {
        from,
        to,
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: pin.subject.clone().unwrap_or_default(),
        body: String::new(),
        in_reply_to: pin.in_reply_to.clone(),
        references,
        attachments: Vec::new(),
        draft_id: Some(pin.id.clone()),
        send_at: None,
        undo_window_seconds: None,
    };
    let payload = serde_json::to_string(&request).map_err(json_to_store_error)?;
    // Account-scope the derived id so it is globally unique even if two accounts
    // stranded a pin under the same id.
    let op_id = format!("recovered-{}-{}", pin.account_id, pin.id);
    tx.execute(
        "INSERT OR IGNORE INTO outbox_operation (
             id, account_id, entity_kind, entity_id, kind, payload,
             payload_version, state, attempts, last_error,
             send_at, hold_until_mono, created_at, updated_at
         )
         VALUES (?1, ?2, 'draft', ?3, 'draftCreate', ?4, 1, 'failed', 0, NULL,
                 NULL, NULL, ?5, ?5)",
        params![op_id, pin.account_id, pin.id, payload, pin.received_at],
    )
    .map_err(sql_to_store_error)?;
    // Self-map the stable key to the pin id so `op_row_touches` resolves the
    // draft op back to its own row.
    tx.execute(
        "INSERT OR IGNORE INTO draft_alias (account_id, draft_key, entity_id)
         VALUES (?1, ?2, ?2)",
        params![pin.account_id, pin.id],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Delete an overlay pin and its overlay set rows (mailbox + keyword
/// memberships), account-scoped by id.
fn delete_overlay_pin(tx: &Connection, account_id: &str, pin_id: &str) -> Result<(), StoreError> {
    for (table, id_column) in [
        ("message_mailbox_overlay", "message_id"),
        ("message_keyword_overlay", "message_id"),
        ("message_overlay", "id"),
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE account_id = ?1 AND {id_column} = ?2"),
            params![account_id, pin_id],
        )
        .map_err(sql_to_store_error)?;
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
