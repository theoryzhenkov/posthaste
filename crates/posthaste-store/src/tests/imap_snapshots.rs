use super::*;

// spec: docs/L0-testing#store-reconciliation-contracts
// spec: docs/L1-sync#imap-locations
// spec: docs/L1-sync#message-snapshot-authoritative
// spec: docs/L1-sync#gmail-label-canonicalization
#[test]
fn full_imap_snapshot_prunes_stale_location_without_deleting_canonical_message(
) -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("imap:gmail:rfc822msgid:canonical");
    let sent_id = MailboxId::from("imap:sent");
    let starred_id = MailboxId::from("imap:starred");
    let sent_mailbox = posthaste_domain::MailboxRecord {
        id: sent_id.clone(),
        name: "Sent".to_string(),
        role: Some("sent".to_string()),
        unread_emails: 0,
        total_emails: 0,
    };
    let starred_mailbox = posthaste_domain::MailboxRecord {
        id: starred_id.clone(),
        name: "Starred".to_string(),
        role: None,
        unread_emails: 0,
        total_emails: 0,
    };
    let sent_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: sent_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(12),
        modseq: Some(ImapModSeq(90)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let starred_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: starred_id.clone(),
        uid_validity: ImapUidValidity(8),
        uid: ImapUid(44),
        modseq: Some(ImapModSeq(91)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let canonical_message = MessageRecord {
        id: message_id.clone(),
        mailbox_ids: vec![sent_id.clone(), starred_id],
        keywords: vec!["$flagged".to_string(), "$seen".to_string()],
        ..sample_message(message_id.as_str(), sent_id.as_str(), Some("mime"))
    };

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![sent_mailbox.clone(), starred_mailbox.clone()],
            messages: vec![canonical_message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![sent_location.clone(), starred_location],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-1", "2026-04-25T00:00:00Z")],
        },
    )?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![sent_mailbox, starred_mailbox],
            messages: vec![MessageRecord {
                mailbox_ids: vec![sent_id],
                keywords: vec!["$seen".to_string()],
                ..sample_message(message_id.as_str(), "imap:sent", Some("mime"))
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![sent_location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-2", "2026-04-25T00:05:00Z")],
        },
    )?;

    assert!(store.get_message_detail(&account, &message_id)?.is_some());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![sent_location]
    );
    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        vec![MailboxId::from("imap:sent")]
    );
    Ok(())
}

// spec: docs/L0-testing#store-reconciliation-contracts
// spec: docs/L1-sync#syncbatch-and-apply_sync_batch
#[test]
fn partial_imap_location_delete_removes_only_that_mailbox_membership() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("imap:gmail:rfc822msgid:canonical");
    let archive_id = MailboxId::from("imap:archive");
    let inbox_id = MailboxId::from("imap:inbox");
    let archive_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: archive_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(12),
        modseq: Some(ImapModSeq(90)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let inbox_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: inbox_id.clone(),
        uid_validity: ImapUidValidity(8),
        uid: ImapUid(44),
        modseq: Some(ImapModSeq(91)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let message = MessageRecord {
        id: message_id.clone(),
        mailbox_ids: vec![archive_id.clone(), inbox_id.clone()],
        ..sample_message(message_id.as_str(), archive_id.as_str(), Some("mime"))
    };

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: archive_id.clone(),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain::MailboxRecord {
                    id: inbox_id.clone(),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
            ],
            messages: vec![message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![archive_location.clone(), inbox_location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: Vec::new(),
        },
    )?;

    let events = store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: vec![inbox_location.key()],
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert!(store.get_message_detail(&account, &message_id)?.is_some());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![archive_location]
    );
    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        vec![archive_id]
    );
    assert!(events.iter().any(|event| {
        event.topic == EVENT_TOPIC_MESSAGE_UPDATED
            && event.payload["changes"]["mailboxes"] == true
            && event.payload["removedMailboxId"] == inbox_id.as_str()
    }));
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .into_iter()
            .filter(|event| event.payload["removedMailboxId"] == inbox_id.as_str())
            .count(),
        1
    );
    Ok(())
}
