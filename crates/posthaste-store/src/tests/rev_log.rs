//! Phase 2 `rev_log` + `rev_cursor` store-layer tests.
//!
//! @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract

use super::*;

fn diff_json() -> Value {
    json!({
        "keywords": {"added": ["$seen"], "removed": []},
        "mailboxes": {"added": ["inbox"], "removed": []}
    })
}

#[test]
fn append_then_fetch_returns_the_step() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    let seq = store.append_rev_log_step(
        &account,
        "step-1",
        "msg-1",
        "source-1",
        &diff_json(),
        "2026-06-28T00:00:00Z",
    )?;

    assert_eq!(seq, 1);
    let log = store.fetch_rev_log(&account, None, 100)?;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].step_id, "step-1");
    assert_eq!(log[0].seq, 1);
    assert_eq!(log[0].message_id, "msg-1");
    assert_eq!(log[0].source_id, "source-1");
    assert_eq!(log[0].diff, diff_json());
    Ok(())
}

#[test]
fn append_is_idempotent_on_step_id() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    let seq_a =
        store.append_rev_log_step(&account, "step-1", "msg-1", "source-1", &diff_json(), "t1")?;
    // Re-deliver the same step_id (e.g. a retried forward action).
    let seq_b =
        store.append_rev_log_step(&account, "step-1", "msg-1", "source-1", &diff_json(), "t1")?;

    assert_eq!(seq_a, seq_b, "idempotent append returns the existing seq");
    let log = store.fetch_rev_log(&account, None, 100)?;
    assert_eq!(log.len(), 1, "no duplicate row");
    Ok(())
}

#[test]
fn seq_is_per_account_monotonic() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    let s1 = store.append_rev_log_step(&account, "a", "m", "s", &diff_json(), "t1")?;
    let s2 = store.append_rev_log_step(&account, "b", "m", "s", &diff_json(), "t2")?;
    let s3 = store.append_rev_log_step(&account, "c", "m", "s", &diff_json(), "t3")?;

    assert_eq!([s1, s2, s3], [1, 2, 3]);
    // Per-account: a second account starts at seq 1.
    let other = AccountId::from("secondary");
    let so = store.append_rev_log_step(&other, "x", "m", "s", &diff_json(), "t4")?;
    assert_eq!(so, 1);
    Ok(())
}

#[test]
fn fetch_delta_since_seq() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    for id in ["a", "b", "c"] {
        store.append_rev_log_step(&account, id, "m", "s", &diff_json(), "t")?;
    }

    let delta = store.fetch_rev_log(&account, Some(1), 100)?;
    assert_eq!(delta.len(), 2);
    assert_eq!(delta[0].step_id, "b");
    assert_eq!(delta[1].step_id, "c");

    // Limit caps the fetch.
    let capped = store.fetch_rev_log(&account, None, 2)?;
    assert_eq!(capped.len(), 2);
    assert_eq!(capped[0].step_id, "a");
    Ok(())
}

#[test]
fn cursor_defaults_to_empty() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let cursor = store.get_rev_cursor(&AccountId::from("primary"))?;
    assert_eq!(cursor.cursor_step_id, None);
    assert!(cursor.redo_tail.is_empty());
    Ok(())
}

#[test]
fn set_then_get_cursor_roundtrips() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.set_rev_cursor(&account, Some("step-2"), &["step-3".to_string()])?;
    let cursor = store.get_rev_cursor(&account)?;
    assert_eq!(cursor.cursor_step_id.as_deref(), Some("step-2"));
    assert_eq!(cursor.redo_tail, vec!["step-3".to_string()]);
    Ok(())
}

#[test]
fn cursor_upsert_overwrites() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.set_rev_cursor(&account, Some("step-2"), &["step-3".to_string()])?;
    // Undo: cursor moves down, redo tail grows.
    store.set_rev_cursor(
        &account,
        Some("step-1"),
        &["step-2".to_string(), "step-3".to_string()],
    )?;

    let cursor = store.get_rev_cursor(&account)?;
    assert_eq!(cursor.cursor_step_id.as_deref(), Some("step-1"));
    assert_eq!(
        cursor.redo_tail,
        vec!["step-2".to_string(), "step-3".to_string()]
    );
    // All-undone: cursor = None.
    store.set_rev_cursor(
        &account,
        None,
        &[
            "step-1".to_string(),
            "step-2".to_string(),
            "step-3".to_string(),
        ],
    )?;
    let cursor = store.get_rev_cursor(&account)?;
    assert_eq!(cursor.cursor_step_id, None);
    assert_eq!(cursor.redo_tail.len(), 3);
    Ok(())
}

#[test]
fn evict_oldest_caps_the_log() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    for id in ["a", "b", "c", "d", "e"] {
        store.append_rev_log_step(&account, id, "m", "s", &diff_json(), "t")?;
    }
    assert_eq!(store.fetch_rev_log(&account, None, 100)?.len(), 5);

    let deleted = store.evict_oldest_rev_log(&account, 3)?;
    assert_eq!(deleted, 2);

    let log = store.fetch_rev_log(&account, None, 100)?;
    assert_eq!(log.len(), 3, "log capped to 3");
    // The oldest two (a, b) are gone; the newest three (c, d, e) remain.
    assert_eq!(log[0].step_id, "c");
    assert_eq!(log[2].step_id, "e");

    // Eviction is a no-op when already under the cap.
    let deleted_again = store.evict_oldest_rev_log(&account, 3)?;
    assert_eq!(deleted_again, 0);
    Ok(())
}
