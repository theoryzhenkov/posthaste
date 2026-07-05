//! M22 (D62) — store close + WAL checkpoint, and the N14 orphan-`.eml` staging
//! guard. Real SQLite in a tempdir; each test cleans its own root (P6).

use super::*;

/// Removes the test's tempdir on drop so a panicking or early-returning test
/// still leaves no state behind (P6).

fn fetched_body(raw_mime: Option<&str>) -> FetchedBody {
    FetchedBody {
        body_html: Some("<p>hi</p>".to_string()),
        body_text: Some("hi".to_string()),
        raw_mime: raw_mime.map(str::to_string),
        attachments: Vec::new(),
    }
}

/// Counts `.eml` body files anywhere under `root`.
fn count_eml_files(root: &std::path::Path) -> usize {
    fn walk(dir: &std::path::Path, count: &mut usize) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("eml") {
                *count += 1;
            }
        }
    }
    let mut count = 0;
    walk(root, &mut count);
    count
}

/// The WAL is checkpointed (truncated to zero, or removed) by `close`.
#[test]
fn close_truncates_the_wal() -> Result<(), StoreError> {
    let root = crate::test_support::temp_root();
    let db_path = root.join("mail.sqlite");
    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
        "message-1",
    )?;

    let wal_path = root.join("mail.sqlite-wal");
    assert!(
        wal_path.exists() && fs::metadata(&wal_path).unwrap().len() > 0,
        "WAL should have grown from the writes before close"
    );

    store.close();

    let truncated = !wal_path.exists() || fs::metadata(&wal_path).unwrap().len() == 0;
    assert!(truncated, "WAL should be truncated or absent after close");
    Ok(())
}

/// A rolled-back write transaction leaves no orphaned body file: staging a body
/// for a message that then fails to apply removes the `.eml`.
#[test]
fn rolled_back_txn_leaves_no_orphan_body() -> Result<(), StoreError> {
    let root = crate::test_support::temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    // The message does not exist, so `apply_message_body` stages the `.eml` then
    // rolls back when the txn fails — the guard must sweep the staged file.
    let result = store.apply_message_body(
        &account,
        &MessageId::from("ghost"),
        &fetched_body(Some("orphan mime")),
    );
    assert!(
        result.is_err(),
        "applying a body to a missing message should fail and roll back"
    );
    assert_eq!(
        count_eml_files(&root.join("data")),
        0,
        "a rolled-back txn must leave no orphan .eml on disk"
    );
    Ok(())
}

/// A staged body that commits with its transaction survives on disk.
#[test]
fn committed_body_survives() -> Result<(), StoreError> {
    let root = crate::test_support::temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    // Seed a metadata-only message (no raw_mime, no body) so seeding stages no
    // `.eml`; the body we apply below is the only file on disk.
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message("message-1", "inbox")],
        "message-1",
    )?;
    assert_eq!(
        count_eml_files(&root.join("data")),
        0,
        "seeding a metadata-only message should stage no body"
    );

    store.apply_message_body(
        &account,
        &MessageId::from("message-1"),
        &fetched_body(Some("kept mime")),
    )?;
    assert_eq!(
        count_eml_files(&root.join("data")),
        1,
        "a committed body must survive on disk"
    );
    Ok(())
}

/// `close` is idempotent: a second call is a harmless no-op.
#[test]
fn double_close_is_idempotent() -> Result<(), StoreError> {
    let root = crate::test_support::temp_root();
    let db_path = root.join("mail.sqlite");
    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
        "message-1",
    )?;

    store.close();
    store.close();

    let wal_path = root.join("mail.sqlite-wal");
    let truncated = !wal_path.exists() || fs::metadata(&wal_path).unwrap().len() == 0;
    assert!(
        truncated,
        "WAL should remain truncated after a double close"
    );
    Ok(())
}

/// A write after `close` fails cleanly (a storage failure naming the closed
/// store) rather than panicking.
#[test]
fn post_close_write_errors_cleanly() -> Result<(), StoreError> {
    let root = crate::test_support::temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.close();

    let err = store
        .append_event(&account, "message.updated", None, None, json!({}))
        .expect_err("a write after close should error");
    match err {
        StoreError::Failure(message) => assert!(
            message.contains("closed"),
            "post-close write error should name the closed store, got: {message}"
        ),
        other => panic!("expected a storage failure, got: {other:?}"),
    }
    Ok(())
}
