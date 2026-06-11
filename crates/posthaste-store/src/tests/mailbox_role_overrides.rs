use super::*;

#[test]
fn mailbox_role_override_survives_full_mailbox_snapshot() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let batch = SyncBatch {
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
        cursors: Vec::new(),
    };

    store.apply_sync_batch(&account, &batch)?;
    store.set_mailbox_role_override(
        &account,
        &MailboxId::from("all-mail"),
        Some("archive"),
        None,
    )?;
    store.apply_sync_batch(&account, &batch)?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "all-mail")
            .and_then(|mailbox| mailbox.role.as_deref()),
        Some("archive")
    );
    Ok(())
}

#[test]
fn mailbox_role_override_can_clear_discovered_previous_owner() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let batch = SyncBatch {
        mailboxes: vec![
            posthaste_domain::MailboxRecord {
                id: MailboxId::from("server-archive"),
                name: "Archive".to_string(),
                role: Some("archive".to_string()),
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
        cursors: Vec::new(),
    };

    store.apply_sync_batch(&account, &batch)?;
    store.set_mailbox_role_override(
        &account,
        &MailboxId::from("all-mail"),
        Some("archive"),
        Some(&MailboxId::from("server-archive")),
    )?;
    store.apply_sync_batch(&account, &batch)?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "server-archive")
            .and_then(|mailbox| mailbox.role.as_deref()),
        None
    );
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "all-mail")
            .and_then(|mailbox| mailbox.role.as_deref()),
        Some("archive")
    );
    Ok(())
}

#[test]
fn mailbox_role_override_rejects_duplicate_role_without_clear() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("server-archive"),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
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
            cursors: Vec::new(),
        },
    )?;

    let error = store
        .set_mailbox_role_override(
            &account,
            &MailboxId::from("all-mail"),
            Some("archive"),
            None,
        )
        .expect_err("duplicate role should be rejected");

    assert!(matches!(error, StoreError::Conflict(message) if message.contains("archive")));
    Ok(())
}

#[test]
fn mailbox_role_override_rejects_unsupported_role() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let error = store
        .set_mailbox_role_override(
            &account,
            &MailboxId::from("all-mail"),
            Some("important"),
            None,
        )
        .expect_err("unsupported role should be rejected");

    assert!(matches!(error, StoreError::Conflict(message) if message.contains("important")));
    Ok(())
}
