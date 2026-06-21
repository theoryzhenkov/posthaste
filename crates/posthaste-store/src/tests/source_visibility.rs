use super::*;

/// Messages must remain visible even if the `source_projection` row drifts away
/// (it only supplies a display name; it must never gate visibility). Previously
/// an INNER JOIN hid all of an account's mail until a restart re-seeded the row.
#[test]
fn messages_remain_visible_without_source_projection_row() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;

    // Sanity: visible with the projection present.
    assert_eq!(store.list_messages(&account, None)?.len(), 1);

    // Simulate projection drift (e.g. a non-atomic write or a rebuilt database
    // before startup re-seeds projections).
    store.delete_source_projection(&account)?;

    let visible = store.list_messages(&account, None)?;
    assert_eq!(
        visible.len(),
        1,
        "mail must remain visible without projection"
    );
    // The display name falls back to the account id rather than disappearing.
    assert_eq!(visible[0].source_name, account.as_str());
    Ok(())
}
