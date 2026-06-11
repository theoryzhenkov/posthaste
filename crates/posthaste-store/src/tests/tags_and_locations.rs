use super::*;

#[test]
fn list_tags_returns_user_keywords_with_counts() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                id: MessageId::from("read-newsletter"),
                keywords: vec!["$seen".to_string(), "newsletter".to_string()],
                ..sample_message("read-newsletter", "inbox", Some("mime-read-newsletter"))
            },
            MessageRecord {
                id: MessageId::from("unread-newsletter"),
                keywords: vec![
                    "newsletter".to_string(),
                    "work".to_string(),
                    "".to_string(),
                    "   ".to_string(),
                    "$custom".to_string(),
                ],
                ..sample_message("unread-newsletter", "inbox", Some("mime-unread-newsletter"))
            },
        ],
        "state",
    )?;

    let tags = store.list_tags(&account)?;

    assert_eq!(
        tags,
        vec![
            TagSummary {
                name: "newsletter".to_string(),
                unread_messages: 1,
                total_messages: 2,
            },
            TagSummary {
                name: "work".to_string(),
                unread_messages: 1,
                total_messages: 1,
            },
        ]
    );
    Ok(())
}

#[test]
fn sync_batch_persists_and_deletes_imap_message_locations() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("message-1");
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: Some(ImapModSeq(999)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let mailbox_state = ImapMailboxSyncState {
        mailbox_id: MailboxId::from("imap:inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(10),
        highest_uid: Some(ImapUid(101)),
        highest_modseq: Some(ImapModSeq(999)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

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
            messages: vec![sample_message("message-1", "inbox", Some("mime"))],
            imap_mailbox_states: vec![mailbox_state.clone()],
            imap_message_locations: vec![location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![location]
    );
    assert_eq!(
        store.get_imap_mailbox_state(&account, &MailboxId::from("imap:inbox"))?,
        Some(mailbox_state)
    );

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: vec![message_id.clone()],
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    Ok(())
}
