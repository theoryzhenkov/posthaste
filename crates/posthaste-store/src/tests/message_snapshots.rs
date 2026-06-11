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
