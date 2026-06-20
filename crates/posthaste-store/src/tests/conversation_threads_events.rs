use super::*;

#[test]
fn conversations_follow_jmap_thread_id_not_headers_or_subject() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let first = sample_message("message-1", "inbox", Some("mime-1"));
    let mut second = sample_message("message-2", "inbox", Some("mime-2"));
    second.source_thread_id = ThreadId::from("thread-2");
    second.subject = first.subject.clone();
    second.in_reply_to = first.rfc_message_id.clone();
    second.references = first.rfc_message_id.iter().cloned().collect();

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
            messages: vec![first, second],
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

    let page = store.list_conversations(
        Some(&account),
        None,
        10,
        None,
        ConversationSortField::default(),
        SortDirection::default(),
    )?;

    assert_eq!(page.items.len(), 2);
    Ok(())
}

#[test]
fn arrival_event_only_emits_for_new_mailbox_membership() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let first_batch = SyncBatch {
        mailboxes: vec![
            posthaste_domain::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            },
            posthaste_domain::MailboxRecord {
                id: MailboxId::from("archive"),
                name: "Archive".to_string(),
                role: Some("archive".to_string()),
                unread_emails: 0,
                total_emails: 0,
            },
        ],
        messages: vec![sample_message("message-1", "inbox", Some("mime"))],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Message,
            state: "state-1".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }],
    };
    let second_batch = SyncBatch {
        mailboxes: first_batch.mailboxes.clone(),
        messages: vec![MessageRecord {
            mailbox_ids: vec![MailboxId::from("archive"), MailboxId::from("inbox")],
            ..sample_message("message-1", "inbox", Some("mime"))
        }],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Message,
            state: "state-2".to_string(),
            updated_at: "2026-03-31T10:05:00Z".to_string(),
        }],
    };

    let first_events = store.apply_sync_batch(&account, &first_batch)?;
    let second_events = store.apply_sync_batch(&account, &second_batch)?;

    let first_arrivals: Vec<_> = first_events
        .iter()
        .filter(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .filter(|event| event.payload["changes"]["arrived"] == true)
        .collect();
    let second_arrivals: Vec<_> = second_events
        .iter()
        .filter(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .filter(|event| event.payload["changes"]["arrived"] == true)
        .collect();

    assert_eq!(first_arrivals.len(), 1);
    assert_eq!(
        first_arrivals[0].payload["arrivedMailboxIds"],
        serde_json::json!(["inbox"])
    );
    assert_eq!(second_arrivals.len(), 1);
    assert_eq!(
        second_arrivals[0].payload["arrivedMailboxIds"],
        serde_json::json!(["archive"])
    );
    Ok(())
}
