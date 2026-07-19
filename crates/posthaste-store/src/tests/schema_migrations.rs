// M84 / NS2 Slice 0: the versioned-migration flow. The legacy fixture is a
// CURRENT database synthetically downgraded to v0 (counter columns + counter
// triggers restored, `user_version` zeroed) — the class of test whose absence
// let the effective-views CREATE order break legacy opens during NS1.
use super::*;

fn raw(path: &std::path::Path) -> Connection {
    Connection::open(path).expect("raw sqlite open")
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

fn mailbox_has_column(connection: &Connection, column: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('mailbox') WHERE name = ?1)",
            [column],
            |row| row.get(0),
        )
        .expect("column probe")
}

fn trigger_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .expect("trigger probe")
}

/// Rewind a current-shape database to the pre-M84 (v0) world: counter columns
/// back on `mailbox`, the counter-maintenance triggers present, version 0.
fn downgrade_to_v0(path: &std::path::Path) {
    let connection = raw(path);
    connection
        .execute_batch(
            "ALTER TABLE mailbox ADD COLUMN unread_emails INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE mailbox ADD COLUMN total_emails INTEGER NOT NULL DEFAULT 0;
             CREATE TRIGGER mailbox_counters_message_mailbox_ai
             AFTER INSERT ON message_mailbox BEGIN
                 UPDATE mailbox SET total_emails = total_emails + 1
                  WHERE account_id = new.account_id AND id = new.mailbox_id;
             END;
             CREATE TRIGGER mailbox_counters_message_mailbox_ad
             AFTER DELETE ON message_mailbox BEGIN
                 UPDATE mailbox SET total_emails = total_emails - 1
                  WHERE account_id = old.account_id AND id = old.mailbox_id;
             END;
             CREATE TRIGGER mailbox_counters_message_read_au
             AFTER UPDATE OF is_read ON message BEGIN
                 UPDATE mailbox SET unread_emails = unread_emails WHERE 0;
             END;
             ALTER TABLE outbox_operation ADD COLUMN depends_on TEXT;
             PRAGMA user_version = 0;",
        )
        .expect("synthetic v0 downgrade");
    // A first-outbox-design legacy row parked as `conflicted` (pre-v2), with a
    // chain edge (pre-v3): the read-time fudge that used to recover the state
    // is gone; migration v2 must rewrite it durably, and migration v3 must
    // drop the `depends_on` column.
    connection
        .execute(
            "INSERT INTO outbox_operation (
                 id, account_id, entity_kind, entity_id, kind, payload,
                 state, attempts, last_error, depends_on, send_at,
                 hold_until_mono, payload_version, created_at, updated_at
             ) VALUES ('op-legacy', 'primary', 'message', 'message-1', 'setKeywords',
                       '{}', 'conflicted', 0, NULL, NULL, NULL, NULL, 1,
                       '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("seed legacy conflicted row");
}

#[test]
fn fresh_open_stamps_the_current_schema_version() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    drop(store);
    assert_eq!(user_version(&raw(&db_path)), crate::db::SCHEMA_VERSION);
    Ok(())
}

#[test]
fn legacy_v0_database_migrates_once_on_open() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");

    // Build a working store, seed it, then rewind it to v0.
    {
        let store = DatabaseStore::open(&db_path, root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![sample_message("message-1", "inbox", None)],
            "seed",
        )?;
    }
    downgrade_to_v0(&db_path);

    // Reopen: migration v1 must run — triggers dropped, columns dropped,
    // version stamped — and the store must remain fully functional.
    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    let account = AccountId::from("primary");
    let mailboxes = store.list_mailboxes(&account)?;
    assert!(
        mailboxes
            .iter()
            .any(|mailbox| mailbox.id.as_str() == "inbox" && mailbox.total_emails == 1),
        "migrated store serves live counts: {mailboxes:?}"
    );
    drop(store);

    let connection = raw(&db_path);
    assert_eq!(user_version(&connection), crate::db::SCHEMA_VERSION);
    assert!(!mailbox_has_column(&connection, "unread_emails"));
    assert!(!mailbox_has_column(&connection, "total_emails"));
    for trigger in [
        "mailbox_counters_message_mailbox_ai",
        "mailbox_counters_message_mailbox_ad",
        "mailbox_counters_message_read_au",
    ] {
        assert!(
            !trigger_exists(&connection, trigger),
            "migration v1 drops {trigger}"
        );
    }
    // v2: the legacy `conflicted` row was durably rewritten to `pending` —
    // the strict state parser (no read-time fudge) can read it.
    let state: String = connection
        .query_row(
            "SELECT state FROM outbox_operation WHERE id = 'op-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row present");
    assert_eq!(state, "pending", "migration v2 recovers conflicted rows");
    // v3 (D174): dependency chains are gone — the column with them.
    let has_depends_on: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('outbox_operation') WHERE name = 'depends_on'
             )",
            [],
            |row| row.get(0),
        )
        .expect("column probe");
    assert!(!has_depends_on, "migration v3 drops depends_on");
    Ok(())
}

/// v4: the out-of-order DATE RECEIVED bug. Pre-fix IMAP ingestion stored the
/// Date header with its original UTC offset; TEXT sort then diverged from
/// chronological order (a `…+02:00` value lexically outranks an earlier-alphabet
/// `…Z` value naming a LATER instant). The migration must rewrite every stored
/// value to the canonical `…Z` shape, after which Date-DESC keyset pagination —
/// including across a cursor seam — walks true chronological order.
#[test]
fn v4_normalizes_offset_received_at_and_restores_date_sort_order() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    {
        let store = DatabaseStore::open(&db_path, root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![
                sample_message("m-newest", "inbox", None),
                sample_message("m-mid", "inbox", None),
                sample_message("m-oldest", "inbox", None),
            ],
            "seed",
        )?;
    }
    {
        // Rewind to v3 with the legacy mixed-offset values. Chronologically
        // (UTC): m-newest 11:40Z > m-mid 11:23Z > m-oldest 04:32Z. Lexically
        // the offset forms sort "13:23:00+02:00" > "11:40:00Z" >
        // "06:32:00+02:00" — m-mid outranks m-newest, the dogfood shape.
        let connection = raw(&db_path);
        for (id, legacy) in [
            ("m-newest", "2026-07-17T11:40:00Z"),
            ("m-mid", "2026-07-17T13:23:00+02:00"),
            ("m-oldest", "2026-07-17T06:32:00+02:00"),
        ] {
            connection
                .execute(
                    "UPDATE message SET received_at = ?1 WHERE id = ?2",
                    [legacy, id],
                )
                .expect("plant legacy offset value");
        }
        connection
            .execute(
                "UPDATE conversation SET latest_received_at = '2026-07-17T13:23:00+02:00'",
                [],
            )
            .expect("plant legacy conversation value");
        connection
            .pragma_update(None, "user_version", 3)
            .expect("rewind to v3");
    }

    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    let account = AccountId::from("primary");
    // Walk Date-DESC one row per page so every boundary is a cursor seam.
    let mut order = Vec::new();
    let mut cursor = None;
    loop {
        let page = store.list_message_page(
            &account,
            None,
            1,
            cursor.as_ref(),
            MessageSortField::Date,
            SortDirection::Desc,
        )?;
        order.extend(page.items.iter().map(|item| item.id.as_str().to_string()));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        order,
        vec!["m-newest", "m-mid", "m-oldest"],
        "Date DESC across cursor seams must be chronological after v4"
    );
    drop(store);

    let connection = raw(&db_path);
    assert_eq!(user_version(&connection), crate::db::SCHEMA_VERSION);
    for (table, column) in [
        ("message", "received_at"),
        ("message_overlay", "received_at"),
        ("conversation", "latest_received_at"),
    ] {
        let stragglers: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} NOT LIKE '%Z'"),
                [],
                |row| row.get(0),
            )
            .expect("straggler probe");
        assert_eq!(stragglers, 0, "v4 normalizes {table}.{column}");
    }
    let normalized: String = connection
        .query_row(
            "SELECT received_at FROM message WHERE id = 'm-mid'",
            [],
            |row| row.get(0),
        )
        .expect("m-mid present");
    assert_eq!(normalized, "2026-07-17T11:23:00Z", "+02:00 folded into UTC");
    Ok(())
}

#[test]
fn downgrade_guard_refuses_a_newer_database_without_quarantining_it() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    drop(DatabaseStore::open(&db_path, root.join("data"))?);
    raw(&db_path)
        .pragma_update(None, "user_version", 9999)
        .expect("bump version");

    let error = DatabaseStore::open_with_repair(&db_path, root.join("data"))
        .map(|_| ())
        .expect_err("a newer database must refuse to open");
    assert!(
        matches!(error, StoreError::Conflict(_)),
        "downgrade guard must be Conflict (never Corruption): {error:?}"
    );
    // The file must be untouched — the repair path must NOT quarantine a
    // database written by a newer build.
    assert!(db_path.exists(), "newer database left in place");
    assert_eq!(
        user_version(&raw(&db_path)),
        9999,
        "newer database unmodified"
    );
    Ok(())
}
