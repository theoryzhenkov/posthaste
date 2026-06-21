use std::fs;

use super::*;
use crate::REPAIR_MARKER_FILE;

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
