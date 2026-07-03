//! Coverage for the deferred body-cache repair (RFC-L2-lifecycle N15 / D67(b)
//! / M27 sub-unit (b)): the three correlated `NOT EXISTS` scans that used to
//! run unconditionally inside `init_schema` on every `DatabaseStore::open`
//! now only run when [`DatabaseStore::repair_body_cache_objects`] is called
//! explicitly.

use super::*;

fn structural_cache_object_exists(store: &DatabaseStore, message_id: &str) -> bool {
    store
        .write_transaction(|tx| {
            let exists = tx
                .query_row(
                    "SELECT 1 FROM cache_object
                     WHERE account_id = 'primary' AND message_id = ?1
                       AND layer = 'body' AND object_id = ''",
                    params![message_id],
                    |_row| Ok(()),
                )
                .optional()
                .map_err(sql_to_store_error)?
                .is_some();
            Ok(exists)
        })
        .expect("query should not fail")
}

#[test]
fn open_no_longer_repairs_missing_body_cache_objects_on_its_own() {
    let root = temp_root();
    let store =
        DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).expect("store opens");

    // A message row inserted directly (bypassing the normal cache_object
    // bookkeeping) is exactly the "missing structural body cache object"
    // shape the repair scan targets.
    insert_message_metadata(&store, "message-1", "2026-01-01T00:00:00Z").expect("insert message");
    assert!(
        !structural_cache_object_exists(&store, "message-1"),
        "the freshly inserted message has no cache_object row yet"
    );

    // Re-opening the store (the old code path ran the repair unconditionally
    // inside `init_schema` on every open) must NOT repair it anymore — the
    // scan has moved off that path.
    drop(store);
    let store =
        DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).expect("store reopens");
    assert!(
        !structural_cache_object_exists(&store, "message-1"),
        "open() must not run the body-cache repair scan anymore"
    );

    // The explicit, deferred repair call still fixes it.
    store
        .repair_body_cache_objects()
        .expect("repair should succeed");
    assert!(
        structural_cache_object_exists(&store, "message-1"),
        "repair_body_cache_objects() should backfill the missing cache_object row"
    );
}

#[test]
fn repair_body_cache_objects_is_idempotent() {
    let root = temp_root();
    let store =
        DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).expect("store opens");
    insert_message_metadata(&store, "message-1", "2026-01-01T00:00:00Z").expect("insert message");

    store
        .repair_body_cache_objects()
        .expect("first repair should succeed");
    store
        .repair_body_cache_objects()
        .expect("second repair should be a no-op, not an error");
    assert!(structural_cache_object_exists(&store, "message-1"));
}
