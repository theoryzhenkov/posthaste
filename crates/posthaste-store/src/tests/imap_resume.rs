use super::*;

// B4: resumable, interruption-safe initial IMAP sync.
//
// These tests drive the store the way the streamed gateway
// (`posthaste_imap::gateway::streaming`) does: each initial-snapshot chunk is an
// UPSERT-ONLY `SyncBatch` carrying that chunk's message rows + IMAP locations and
// an advancing `ImapMailboxSyncState` checkpoint (`partial_initial_uid`), and
// `replace_all_messages` is NEVER set. The finalizing chunk clears the checkpoint
// and writes the completed watermark. This is exactly the store contract the
// gateway relies on, so testing it here proves the resume + DS1 invariants
// without a live IMAP protocol.
//
// spec: docs/L0-testing#store-reconciliation-contracts
// spec: docs/L1-sync#syncbatch-and-apply_sync_batch

const INBOX: &str = "imap:inbox";
const UID_VALIDITY: ImapUidValidity = ImapUidValidity(7);

fn inbox_mailbox() -> posthaste_domain_model::MailboxRecord {
    posthaste_domain_model::MailboxRecord {
        id: MailboxId::from(INBOX),
        name: "INBOX".to_string(),
        role: Some("inbox".to_string()),
        unread_emails: 0,
        total_emails: 0,
    }
}

/// The mailbox snapshot the gateway emits once, before any message chunk
/// (authoritative mailbox set; upsert-only for messages).
fn mailbox_snapshot_batch() -> SyncBatch {
    SyncBatch {
        mailboxes: vec![inbox_mailbox()],
        replace_all_mailboxes: true,
        ..SyncBatch::default()
    }
}

fn message_id_for(uid: u32) -> MessageId {
    MessageId::from(format!("imap:{}:{}", UID_VALIDITY.0, uid))
}

fn location_for(uid: u32) -> ImapMessageLocation {
    ImapMessageLocation {
        message_id: message_id_for(uid),
        mailbox_id: MailboxId::from(INBOX),
        uid_validity: UID_VALIDITY,
        uid: ImapUid(uid),
        modseq: Some(ImapModSeq(u64::from(uid))),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}

fn partial_state(checkpoint: u32) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: MailboxId::from(INBOX),
        mailbox_name: "INBOX".to_string(),
        uid_validity: UID_VALIDITY,
        highest_uid: None,
        highest_modseq: None,
        partial_initial_uid: Some(ImapUid(checkpoint)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}

fn finalized_state(highest_uid: u32) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: MailboxId::from(INBOX),
        mailbox_name: "INBOX".to_string(),
        uid_validity: UID_VALIDITY,
        highest_uid: Some(ImapUid(highest_uid)),
        highest_modseq: Some(ImapModSeq(u64::from(highest_uid))),
        partial_initial_uid: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}

/// One upsert-only initial-snapshot chunk: message rows + locations for `uids`
/// plus the advancing checkpoint `state`. Never sets `replace_all_messages`.
fn chunk_batch(uids: &[u32], state: ImapMailboxSyncState) -> SyncBatch {
    SyncBatch {
        messages: uids
            .iter()
            .map(|uid| {
                let id = message_id_for(*uid);
                MessageRecord {
                    id: id.clone(),
                    ..sample_message(id.as_str(), INBOX, Some("mime"))
                }
            })
            .collect(),
        imap_message_locations: uids.iter().map(|uid| location_for(*uid)).collect(),
        imap_mailbox_states: vec![state],
        replace_all_mailboxes: false,
        replace_all_messages: false,
        ..SyncBatch::default()
    }
}

fn present_uids(store: &DatabaseStore, account: &AccountId) -> Result<Vec<u32>, StoreError> {
    Ok(store
        .list_imap_mailbox_message_locations(account, &MailboxId::from(INBOX))?
        .into_iter()
        .map(|location| location.uid.0)
        .collect())
}

#[test]
fn interrupted_initial_sync_resumes_from_committed_cursor_without_dup_or_skip(
) -> Result<(), StoreError> {
    // 10 messages arriving as 5 chunks of 2 (UIDs 1..=10). The sync is
    // interrupted after chunk 2; a restart resumes from the committed cursor and
    // must reach the SAME final state as an uninterrupted sync — no missing, no
    // duplicated messages.
    let chunks: [[u32; 2]; 5] = [[1, 2], [3, 4], [5, 6], [7, 8], [9, 10]];

    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    // A local sentinel that the "server" no longer has: it is absent from every
    // chunk. Upsert-only checkpoints must never prune it (DS1). Seeded via a
    // plain upsert so it starts present.
    let sentinel = MessageId::from("imap:7:sentinel");
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![inbox_mailbox()],
            messages: vec![MessageRecord {
                id: sentinel.clone(),
                ..sample_message(sentinel.as_str(), INBOX, Some("mime"))
            }],
            imap_message_locations: vec![ImapMessageLocation {
                message_id: sentinel.clone(),
                mailbox_id: MailboxId::from(INBOX),
                uid_validity: UID_VALIDITY,
                uid: ImapUid(999),
                modseq: Some(ImapModSeq(999)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            replace_all_mailboxes: true,
            replace_all_messages: false,
            ..SyncBatch::default()
        },
    )?;

    // Mailbox snapshot, then the first two chunks (checkpoint after each).
    store.apply_sync_batch(&account, &mailbox_snapshot_batch())?;
    store.apply_sync_batch(&account, &chunk_batch(&chunks[0], partial_state(2)))?;
    store.apply_sync_batch(&account, &chunk_batch(&chunks[1], partial_state(4)))?;

    // --- Interrupted here (chunk 2 of 5 committed) ---
    let mid = store
        .get_imap_mailbox_state(&account, &MailboxId::from(INBOX))?
        .expect("mailbox state after chunk 2");
    assert_eq!(
        mid.partial_initial_uid,
        Some(ImapUid(4)),
        "the durable cursor advanced to the highest committed UID",
    );
    assert_eq!(
        mid.highest_uid, None,
        "no completed watermark while the snapshot is still in progress",
    );
    assert_eq!(
        present_uids(&store, &account)?,
        vec![1, 2, 3, 4, 999],
        "chunks 1-2 are durably committed (and the sentinel survives)",
    );
    assert!(
        store.get_message_detail(&account, &sentinel)?.is_some(),
        "an upsert-only checkpoint must NOT prune the absent sentinel (DS1)",
    );

    // --- Resume: the planner would pick ResumeInitialSync with after_uid=4, so
    // only chunks 3-5 are fetched. Chunk 5 finalizes the snapshot. ---
    store.apply_sync_batch(&account, &chunk_batch(&chunks[2], partial_state(6)))?;
    store.apply_sync_batch(&account, &chunk_batch(&chunks[3], partial_state(8)))?;
    store.apply_sync_batch(&account, &chunk_batch(&chunks[4], finalized_state(10)))?;

    let done = store
        .get_imap_mailbox_state(&account, &MailboxId::from(INBOX))?
        .expect("mailbox state after completion");
    assert_eq!(
        done.partial_initial_uid, None,
        "completion clears the resumable checkpoint",
    );
    assert_eq!(
        done.highest_uid,
        Some(ImapUid(10)),
        "completion writes the authoritative watermark",
    );

    // Every message present exactly once, none skipped, none duplicated — plus
    // the never-pruned sentinel.
    assert_eq!(
        present_uids(&store, &account)?,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 999],
    );
    assert!(store.get_message_detail(&account, &sentinel)?.is_some());

    // The interrupted-then-resumed run reaches the SAME final state as an
    // uninterrupted single-batch initial sync of the same 10 messages.
    let root2 = temp_root();
    let store2 = DatabaseStore::open(root2.join("mail.sqlite"), root2.join("data"))?;
    setup_source(&store2, &account, "Primary")?;
    store2.apply_sync_batch(&account, &mailbox_snapshot_batch())?;
    let all: Vec<u32> = (1..=10).collect();
    store2.apply_sync_batch(&account, &chunk_batch(&all, finalized_state(10)))?;
    assert_eq!(
        present_uids(&store2, &account)?,
        (1..=10).collect::<Vec<_>>(),
        "uninterrupted baseline: the 10 messages, no sentinel",
    );
    assert_eq!(
        store2.get_imap_mailbox_state(&account, &MailboxId::from(INBOX))?,
        store.get_imap_mailbox_state(&account, &MailboxId::from(INBOX))?,
        "resumed and uninterrupted syncs converge on the same mailbox state",
    );

    Ok(())
}

#[test]
fn resumed_chunk_replay_is_idempotent_by_uid() -> Result<(), StoreError> {
    // If a chunk is re-applied (an at-least-once replay across the interruption
    // boundary), the UPSERT-by-UID keeps exactly one row — no duplicates.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(&account, &mailbox_snapshot_batch())?;
    store.apply_sync_batch(&account, &chunk_batch(&[1, 2], partial_state(2)))?;
    // Replay chunk 1-2 (as a crash-and-resume might), then continue.
    store.apply_sync_batch(&account, &chunk_batch(&[1, 2], partial_state(2)))?;
    store.apply_sync_batch(&account, &chunk_batch(&[3, 4], finalized_state(4)))?;

    assert_eq!(present_uids(&store, &account)?, vec![1, 2, 3, 4]);
    Ok(())
}
