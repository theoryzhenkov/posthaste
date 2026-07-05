use super::*;

#[test]
fn account_scoped_reads_do_not_leak() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account_a = AccountId::from("primary");
    let account_b = AccountId::from("secondary");
    setup_source(&store, &account_a, "Primary")?;
    setup_source(&store, &account_b, "Secondary")?;

    store.apply_sync_batch(
        &account_a,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_model::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![sample_message("shared-id", "inbox", Some("mime-a"))],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "a".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    )?;
    store.apply_sync_batch(
        &account_b,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_model::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![sample_message("shared-id", "inbox", Some("mime-b"))],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "b".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    )?;

    let detail_a = store
        .get_message_detail(&account_a, &MessageId::from("shared-id"))?
        .unwrap();
    let detail_b = store
        .get_message_detail(&account_b, &MessageId::from("shared-id"))?
        .unwrap();
    assert_ne!(
        detail_a.raw_message.as_ref().unwrap().path,
        detail_b.raw_message.as_ref().unwrap().path
    );
    Ok(())
}

#[test]
fn message_detail_preserves_recipients() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut message = sample_message("sent-message", "sent", Some("mime"));
    message.to = vec![Recipient {
        name: Some("Bob Recipient".to_string()),
        email: "bob@example.com".to_string(),
    }];
    seed_messages(&store, &account, vec![message], "state-1")?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("sent-message"))?
        .unwrap();
    assert_eq!(detail.summary.to.len(), 1);
    assert_eq!(detail.summary.to[0].name.as_deref(), Some("Bob Recipient"));
    assert_eq!(detail.summary.to[0].email, "bob@example.com");
    Ok(())
}

#[test]
fn sync_batch_is_atomic_when_junction_insert_fails() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let result = store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_model::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![MessageRecord {
                mailbox_ids: vec![MailboxId::from("inbox"), MailboxId::from("inbox")],
                ..sample_message("message-1", "inbox", Some("mime"))
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    );
    assert!(result.is_err());
    assert!(store.list_messages(&account, None)?.is_empty());
    assert!(store.get_cursor(&account, SyncObject::Message)?.is_none());
    Ok(())
}

#[test]
fn event_replay_respects_after_seq() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    let first = store.append_event(
        &account,
        EVENT_TOPIC_MESSAGE_UPDATED,
        None,
        None,
        json!({"n": 1}),
    )?;
    let _second = store.append_event(
        &account,
        EVENT_TOPIC_MESSAGE_UPDATED,
        None,
        None,
        json!({"n": 2}),
    )?;

    let events = store.list_events(&EventFilter {
        account_id: Some(account),
        topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
        mailbox_id: None,
        after_seq: Some(first.seq),
    })?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["n"], 2);
    Ok(())
}

#[test]
fn event_replay_compares_after_seq_as_integer() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    for n in 1..=11 {
        store.append_event(
            &account,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            None,
            json!({ "n": n }),
        )?;
    }

    let events = store.list_events(&EventFilter {
        account_id: Some(account),
        topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
        mailbox_id: None,
        after_seq: Some(9),
    })?;

    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![10, 11]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.payload["n"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![10, 11]
    );
    Ok(())
}
