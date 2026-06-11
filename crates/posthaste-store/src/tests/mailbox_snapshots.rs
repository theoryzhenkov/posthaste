use super::*;

#[test]
fn full_mailbox_snapshot_removes_stale_local_mailboxes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("all-mail"),
                    name: "All Mail".to_string(),
                    role: None,
                    unread_emails: 0,
                    total_emails: 0,
                },
            ],
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Mailbox,
                state: "mailbox-1".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    )?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![posthaste_domain::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Mailbox,
                state: "mailbox-2".to_string(),
                updated_at: "2026-03-31T10:05:00Z".to_string(),
            }],
        },
    )?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(mailboxes.len(), 1);
    assert_eq!(mailboxes[0].id, MailboxId::from("inbox"));
    Ok(())
}
