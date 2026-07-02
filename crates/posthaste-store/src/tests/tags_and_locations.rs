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
                    String::new(),
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
fn message_summary_carries_max_modseq_as_version() -> Result<(), StoreError> {
    use posthaste_domain_service::MessageDetailStore;

    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    // An IMAP message present in two mailboxes at different modseqs: the summary
    // version is the max (the message's latest authority state). modseq is
    // stored as TEXT, so this also covers the numeric (not lexical) ordering:
    // 1000 > 999 numerically, but "1000" < "999" lexically.
    let imap_id = MessageId::from("imap-msg");
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_service::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![
                sample_message("imap-msg", "inbox", Some("mime-imap")),
                sample_message("local-msg", "inbox", Some("mime-local")),
            ],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![
                ImapMessageLocation {
                    message_id: imap_id.clone(),
                    mailbox_id: MailboxId::from("imap:inbox"),
                    uid_validity: ImapUidValidity(10),
                    uid: ImapUid(101),
                    modseq: Some(ImapModSeq(999)),
                    updated_at: "2026-04-25T00:00:00Z".to_string(),
                },
                ImapMessageLocation {
                    message_id: imap_id.clone(),
                    mailbox_id: MailboxId::from("imap:all"),
                    uid_validity: ImapUidValidity(10),
                    uid: ImapUid(102),
                    modseq: Some(ImapModSeq(1000)),
                    updated_at: "2026-04-25T00:00:00Z".to_string(),
                },
            ],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    // The IMAP message: version = max(modseq) = 1000.
    let imap_summary = store
        .get_message_summary(&account, &imap_id)?
        .expect("imap message summary");
    assert_eq!(imap_summary.version, Some(1000));

    // A message with no IMAP location (JMAP / mock / local): no version.
    let local_summary = store
        .get_message_summary(&account, &MessageId::from("local-msg"))?
        .expect("local message summary");
    assert_eq!(local_summary.version, None);
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
            mailboxes: vec![posthaste_domain_service::MailboxRecord {
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
