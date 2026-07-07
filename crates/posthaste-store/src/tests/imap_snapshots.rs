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
    let sent_mailbox = posthaste_domain_model::MailboxRecord {
        id: sent_id.clone(),
        name: "Sent".to_string(),
        role: Some("sent".to_string()),
        unread_emails: 0,
        total_emails: 0,
    };
    let starred_mailbox = posthaste_domain_model::MailboxRecord {
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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
                posthaste_domain_model::MailboxRecord {
                    id: archive_id.clone(),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain_model::MailboxRecord {
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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
        vec![archive_id.clone()]
    );
    // The membership-removal event self-maintains the reactive store (option iii):
    // it carries the post-removal projection (mailboxIds = [archive]) instead of
    // being projection-less (which the store dropped, leaving the runtime's
    // per-view re-serve as the only corrector). Counts ride no event
    // (RFC-L2-count-unification): clients invalidate + re-read the canonical
    // mailbox counts.
    let membership_event = events
        .iter()
        .find(|event| {
            event.topic == EVENT_TOPIC_MESSAGE_UPDATED
                && event.payload["changes"]["mailboxes"] == true
                && event.payload["removedMailboxId"] == inbox_id.as_str()
        })
        .expect("membership-change event for the removed inbox location");
    assert_eq!(
        membership_event.payload["projection"]["mailboxIds"],
        serde_json::json!([archive_id.as_str()]),
    );
    assert!(
        membership_event.payload.get("countDeltas").is_none(),
        "no countDeltas on the membership-removal event (invalidation model)",
    );
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

// spec: docs/eph/AUDIT-L2-architecture-health (DP-C4 / H1)
// DP-C4 mail-loss: the IMAP explicit-delete loop must route ABSENCE-derived
// (inferred from a possibly-truncated `UID SEARCH UNDELETED`) deletions through
// the DS1 floor guard, while server-asserted VANISHED deletions bypass it.
fn imap_inbox_location(message_id: &MessageId, uid: u32) -> ImapMessageLocation {
    ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(uid),
        modseq: Some(ImapModSeq(u64::from(90 + uid))),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}

fn seed_imap_inbox_messages(
    store: &DatabaseStore,
    account: &AccountId,
    count: u32,
) -> Result<Vec<(MessageId, ImapMessageLocation)>, StoreError> {
    let mut seeded = Vec::new();
    let mut messages = Vec::new();
    let mut locations = Vec::new();
    for i in 1..=count {
        let id = MessageId::from(format!("imap:msg-{i}"));
        let location = imap_inbox_location(&id, 10 + i);
        messages.push(MessageRecord {
            mailbox_ids: vec![MailboxId::from("imap:inbox")],
            ..sample_message(id.as_str(), "imap:inbox", Some(&format!("mime-{i}")))
        });
        locations.push(location.clone());
        seeded.push((id, location));
    }
    store.apply_sync_batch(
        account,
        &SyncBatch {
            messages,
            imap_message_locations: locations,
            ..SyncBatch::default()
        },
    )?;
    Ok(seeded)
}

#[test]
fn imap_absence_floor_guard_refuses_drastic_absence_deletion() -> Result<(), StoreError> {
    // A truncated/empty `UID SEARCH UNDELETED` makes most of a mailbox's local
    // mail look "absent". BEFORE the guard the explicit-delete loop wiped it;
    // now the absence-derived removals over the floor are refused and local mail
    // survives.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let seeded = seed_imap_inbox_messages(&store, &account, 4)?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            absence_deleted_imap_message_locations: seeded[0..3]
                .iter()
                .map(|(_, location)| location.key())
                .collect(),
            absence_deleted_message_ids: seeded[0..3].iter().map(|(id, _)| id.clone()).collect(),
            ..SyncBatch::default()
        },
    )?;

    assert_eq!(
        store.list_messages(&account, None)?.len(),
        4,
        "absence-derived deletions over the floor must be refused (local mail preserved)",
    );
    Ok(())
}

#[test]
fn imap_vanished_deletions_bypass_the_absence_floor_guard() -> Result<(), StoreError> {
    // A GENUINE VANISHED delete is a server assertion and must still delete, even
    // when it removes more than the floor fraction of the local store.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let seeded = seed_imap_inbox_messages(&store, &account, 4)?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            deleted_imap_message_locations: seeded[0..3]
                .iter()
                .map(|(_, location)| location.key())
                .collect(),
            deleted_message_ids: seeded[0..3].iter().map(|(id, _)| id.clone()).collect(),
            ..SyncBatch::default()
        },
    )?;

    let remaining = store.list_messages(&account, None)?;
    assert_eq!(
        remaining.len(),
        1,
        "server-asserted VANISHED deletions delete unconditionally, past the floor",
    );
    assert_eq!(remaining[0].id, seeded[3].0);
    Ok(())
}

#[test]
fn imap_absence_deletion_below_floor_still_prunes() -> Result<(), StoreError> {
    // The guard must not over-correct: an ordinary single absence-derived
    // deletion (well under the floor) still prunes normally.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let seeded = seed_imap_inbox_messages(&store, &account, 4)?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            absence_deleted_imap_message_locations: vec![seeded[0].1.key()],
            absence_deleted_message_ids: vec![seeded[0].0.clone()],
            ..SyncBatch::default()
        },
    )?;

    assert_eq!(
        store.list_messages(&account, None)?.len(),
        3,
        "a single genuine absence deletion still prunes",
    );
    assert!(
        store.get_message_detail(&account, &seeded[0].0)?.is_none(),
        "the absent message is pruned",
    );
    Ok(())
}
