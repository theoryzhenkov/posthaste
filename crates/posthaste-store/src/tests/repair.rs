use std::fs;

use super::*;
use crate::REPAIR_MARKER_FILE;

fn raw(path: &std::path::Path) -> Connection {
    Connection::open(path).expect("raw sqlite open")
}

/// Seed one `outbox_operation` row via raw SQL (control over the rowid order the
/// salvage must preserve). `rowid` is left to SQLite's auto-increment, so the
/// insertion order here IS the log order.
fn seed_op(connection: &Connection, id: &str, kind: &str, state: &str, entity_id: &str) {
    connection
        .execute(
            "INSERT INTO outbox_operation (
                 id, account_id, entity_kind, entity_id, kind, payload,
                 payload_version, state, attempts, created_at, updated_at
             ) VALUES (?1, 'primary', 'draft', ?2, ?3,
                       '{\"to\":[],\"subject\":\"s\",\"body\":\"words\"}',
                       1, ?4, 0, '2026-05-01T09:00:00Z', '2026-05-01T09:00:00Z')",
            params![id, entity_id, kind, state],
        )
        .expect("seed outbox op");
}

/// A corrupt database file is quarantined and a fresh, usable store is rebuilt.
#[test]
fn open_quarantines_and_rebuilds_a_corrupt_database() -> Result<(), StoreError> {
    let root = temp_root();
    let state_root = root.join("state");
    fs::create_dir_all(&state_root).unwrap();
    let db_path = state_root.join("mail.sqlite");

    // Not a SQLite database: SQLite reports this as corruption on open.
    fs::write(&db_path, b"this is not a sqlite database, it is garbage").unwrap();

    let (store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    let report = report.expect("a repair should have been reported");

    // The corrupt bytes were moved aside, not deleted in place.
    assert!(report.quarantined_path.exists());
    assert!(report
        .quarantined_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains(".corrupt-"));

    // The rebuilt database is usable.
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    Ok(())
}

/// A healthy database opens without any repair report.
#[test]
fn open_healthy_database_reports_no_repair() -> Result<(), StoreError> {
    let root = temp_root();
    let state_root = root.join("state");
    let db_path = state_root.join("mail.sqlite");

    let (_store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    assert!(report.is_none());

    // Reopening the now-existing database also reports no repair.
    let (_store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    assert!(report.is_none());
    Ok(())
}

/// The repair marker forces a rebuild even when the database is healthy, and the
/// marker is consumed.
#[test]
fn repair_marker_forces_rebuild_and_is_consumed() -> Result<(), StoreError> {
    let root = temp_root();
    let state_root = root.join("state");
    let db_path = state_root.join("mail.sqlite");

    // Create a healthy database with some content.
    {
        let (store, _) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
        setup_source(&store, &AccountId::from("primary"), "Primary")?;
    }

    let marker = state_root.join(REPAIR_MARKER_FILE);
    fs::write(&marker, b"").unwrap();

    let (store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    let report = report.expect("marker should force a repair");
    assert_eq!(report.reason, "manual repair requested");
    assert!(report.quarantined_path.exists());
    assert!(!marker.exists(), "marker should be consumed");

    // The rebuilt database is usable (the previous source was quarantined away).
    setup_source(&store, &AccountId::from("primary"), "Primary")?;
    Ok(())
}

/// A repair must SALVAGE the op log across the rebuild: the log is precious (it
/// holds a parked content op — the user's unsent words — and a pending intent),
/// so a marker-forced rebuild carries the outbox_operation rows and their
/// draft_alias identity into the fresh database, in their original log order.
#[test]
fn repair_salvages_the_op_log() -> Result<(), StoreError> {
    let root = temp_root();
    let state_root = root.join("state");
    let db_path = state_root.join("mail.sqlite");

    {
        let (store, _) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
        setup_source(&store, &AccountId::from("primary"), "Primary")?;
    }
    // Seed the log directly, preserving insertion (rowid) order: a parked
    // content op, then a pending intent, then the draft's identity alias.
    {
        let connection = raw(&db_path);
        seed_op(
            &connection,
            "op-parked",
            "draftCreate",
            "failed",
            "draft-key",
        );
        seed_op(&connection, "op-pending", "send", "pending", "send-1");
        connection
            .execute(
                "INSERT INTO draft_alias (account_id, draft_key, entity_id)
                 VALUES ('primary', 'draft-key', 'draft-key')",
                [],
            )
            .expect("seed alias");
    }

    // Force a rebuild via the marker (the file is still readable, so salvage
    // runs and the log survives).
    let marker = state_root.join(REPAIR_MARKER_FILE);
    fs::write(&marker, b"").unwrap();
    let (store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    let report = report.expect("marker forces a repair");
    assert_eq!(
        report.salvaged_operations, 2,
        "both op-log rows are salvaged across the rebuild"
    );
    drop(store);

    // The salvaged rows survived into the rebuilt database, in log order.
    let connection = raw(&db_path);
    let ids: Vec<String> = {
        let mut statement = connection
            .prepare("SELECT id FROM outbox_operation ORDER BY rowid ASC")
            .expect("prepare");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query");
        rows.map(|r| r.expect("row")).collect()
    };
    assert_eq!(
        ids,
        vec!["op-parked".to_string(), "op-pending".to_string()],
        "the salvaged log keeps its insertion order"
    );
    let alias: String = connection
        .query_row(
            "SELECT entity_id FROM draft_alias WHERE draft_key = 'draft-key'",
            [],
            |row| row.get(0),
        )
        .expect("alias survived");
    assert_eq!(alias, "draft-key", "the draft alias identity survived");
    Ok(())
}

/// A file too corrupt to read still opens (empty salvage), is quarantined, and
/// never panics — best-effort salvage never fails the open.
#[test]
fn repair_salvage_tolerates_an_unreadable_file() -> Result<(), StoreError> {
    let root = temp_root();
    let state_root = root.join("state");
    fs::create_dir_all(&state_root).unwrap();
    let db_path = state_root.join("mail.sqlite");
    fs::write(
        &db_path,
        b"not a sqlite database at all, unreadable garbage",
    )
    .unwrap();

    let (store, report) = DatabaseStore::open_with_repair(&db_path, &state_root)?;
    let report = report.expect("a repair should have been reported");
    assert_eq!(
        report.salvaged_operations, 0,
        "an unreadable file yields an empty salvage"
    );
    // The bytes are retained on disk for manual recovery.
    assert!(report.quarantined_path.exists());
    // The rebuilt database is usable.
    setup_source(&store, &AccountId::from("primary"), "Primary")?;
    Ok(())
}
