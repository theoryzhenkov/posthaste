use super::*;

#[test]
fn full_message_snapshot_removes_stale_local_messages() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mailbox = posthaste_domain::MailboxRecord {
        id: MailboxId::from("inbox"),
        name: "Inbox".to_string(),
        role: Some("inbox".to_string()),
        unread_emails: 0,
        total_emails: 0,
    };
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![mailbox.clone()],
            messages: vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("message-2", "inbox", Some("mime-2")),
            ],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-1", "2026-03-31T10:00:00Z")],
        },
    )?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![mailbox],
            messages: vec![sample_message("message-2", "inbox", Some("mime-2"))],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-2", "2026-03-31T10:05:00Z")],
        },
    )?;

    let messages = store.list_messages(&account, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, MessageId::from("message-2"));
    assert!(store
        .get_message_detail(&account, &MessageId::from("message-1"))?
        .is_none());
    Ok(())
}

#[test]
fn message_draft_id_round_trips_through_apply_and_detail() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut draft = sample_message("draft-1", "drafts", Some("draft-mime"));
    draft.draft_id = Some("draft-local-stable".to_string());
    let plain = sample_message("message-1", "inbox", Some("mime-1"));

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![draft, plain],
            ..SyncBatch::default()
        },
    )?;

    // The stable draft identity survives apply + read; a non-draft message has none.
    let draft_detail = store
        .get_message_detail(&account, &MessageId::from("draft-1"))?
        .expect("draft detail");
    assert_eq!(draft_detail.draft_id.as_deref(), Some("draft-local-stable"));
    let plain_detail = store
        .get_message_detail(&account, &MessageId::from("message-1"))?
        .expect("message detail");
    assert_eq!(plain_detail.draft_id, None);
    Ok(())
}
