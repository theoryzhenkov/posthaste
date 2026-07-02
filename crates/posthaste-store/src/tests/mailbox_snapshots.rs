use super::*;

#[test]
fn mailbox_counters_track_membership_and_read_state_incrementally() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut unread = sample_message("m-unread", "inbox", Some("mime-unread"));
    unread.keywords = Vec::new();
    seed_messages(
        &store,
        &account,
        vec![unread, sample_message("m-read", "inbox", Some("mime-read"))],
        "state-1",
    )?;
    assert_mailbox_counts(&store, &account, "inbox", 2, 1)?;

    store.set_keywords(
        &account,
        &MessageId::from("m-unread"),
        None,
        &SetKeywordsCommand {
            add: vec!["$seen".to_string()],
            remove: Vec::new(),
        },
    )?;
    assert_mailbox_counts(&store, &account, "inbox", 2, 0)?;

    store.replace_mailboxes(
        &account,
        &MessageId::from("m-read"),
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;
    assert_mailbox_counts(&store, &account, "inbox", 1, 0)?;
    assert_mailbox_counts(&store, &account, "archive", 1, 0)?;

    store.destroy_message(&account, &MessageId::from("m-unread"), None)?;
    assert_mailbox_counts(&store, &account, "inbox", 0, 0)?;

    Ok(())
}

fn assert_mailbox_counts(
    store: &DatabaseStore,
    account: &AccountId,
    mailbox_id: &str,
    total: i64,
    unread: i64,
) -> Result<(), StoreError> {
    let mailboxes = store.list_mailboxes(account)?;
    let mailbox = mailboxes
        .iter()
        .find(|mailbox| mailbox.id == MailboxId::from(mailbox_id))
        .unwrap_or_else(|| panic!("missing mailbox {mailbox_id}"));
    assert_eq!(mailbox.total_emails, total, "total count for {mailbox_id}");
    assert_eq!(
        mailbox.unread_emails, unread,
        "unread count for {mailbox_id}"
    );
    Ok(())
}

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
                posthaste_domain_model::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain_model::MailboxRecord {
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
            mailboxes: vec![posthaste_domain_model::MailboxRecord {
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
