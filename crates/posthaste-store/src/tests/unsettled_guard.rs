// Store-level coverage for the M35 durable snapshot guard's prune exemption
// (D93 — supersedes the P1/S2 hotfix): `apply_sync_batch_protected`/
// `reconcile_sync_protected` must exclude `protected_message_ids` from the
// prune-by-absence pass, while still pruning every other locally-absent message
// normally. These tests exercise the store's half of the guard — the exemption
// for messages a full snapshot *omits*; the domain-service `guard_unsettled`
// folds an un-acked op over any row the snapshot *does* carry (tested there).
use super::*;

#[test]
fn full_snapshot_protects_a_pending_local_create_from_prune() -> Result<(), StoreError> {
    // P1: a locally-created message still pending (e.g. draft/send in flight,
    // or an optimistic create not yet observed by the provider) was never in
    // any remote snapshot to begin with — plain `apply_sync_batch` would prune
    // it as "absent from remote" on the very next full IMAP resync.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("local-only", "inbox", Some("mime-local")),
            ],
            ..SyncBatch::default()
        },
    )?;

    // Full snapshot: the remote only ever had message-1. "local-only" is a
    // pending, not-yet-uploaded local message, so it is absent here too — the
    // guard's protected set is the only thing standing between it and prune.
    let protected = HashSet::from(["local-only".to_string()]);
    store.apply_sync_batch_protected(
        &account,
        &SyncBatch {
            messages: vec![sample_message("message-1", "inbox", Some("mime-1"))],
            replace_all_messages: true,
            cursors: vec![message_cursor("state-1", "2026-03-31T10:05:00Z")],
            ..SyncBatch::default()
        },
        &protected,
    )?;

    let messages = store.list_messages(&account, None)?;
    let ids: std::collections::BTreeSet<_> = messages.iter().map(|m| m.id.clone()).collect();
    assert_eq!(
        ids,
        std::collections::BTreeSet::from([
            MessageId::from("message-1"),
            MessageId::from("local-only"),
        ]),
        "the protected local-only message survives the full-snapshot prune pass",
    );
    Ok(())
}

#[test]
fn full_snapshot_still_prunes_unprotected_messages_absent_from_remote() -> Result<(), StoreError> {
    // No over-protection: a message with no unsettled op is pruned exactly as
    // before once it is genuinely absent from a full remote snapshot.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("message-2", "inbox", Some("mime-2")),
            ],
            ..SyncBatch::default()
        },
    )?;

    // message-2 is unsettled and protected; message-1 has no op and is simply
    // gone from the remote snapshot (deleted remotely) — it must still prune.
    let protected = HashSet::from(["message-2".to_string()]);
    store.apply_sync_batch_protected(
        &account,
        &SyncBatch {
            messages: Vec::new(),
            replace_all_messages: true,
            cursors: vec![message_cursor("state-1", "2026-03-31T10:05:00Z")],
            ..SyncBatch::default()
        },
        &protected,
    )?;

    let messages = store.list_messages(&account, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, MessageId::from("message-2"));
    Ok(())
}

#[test]
fn full_snapshot_leaves_a_protected_messages_local_state_untouched() -> Result<(), StoreError> {
    // S2: a pending local mailbox/flag change must not be visibly reverted by a
    // full-snapshot upsert. This test covers the case where the message is
    // *absent* from the server snapshot (so `guard_unsettled` leaves it out of
    // `batch.messages`); the store must then also skip pruning it, so the local
    // row — mailbox move and keyword both included — survives exactly as the
    // optimistic write left it. (When the snapshot *does* carry the row, the
    // service folds server-truth-plus-pending in-batch instead — tested there.)
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut locally_archived = sample_message("message-1", "archive", Some("mime-1"));
    locally_archived.keywords = vec!["$seen".to_string(), "$flagged".to_string()];
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![locally_archived],
            ..SyncBatch::default()
        },
    )?;

    // The full snapshot's view of message-1 (still in the inbox, unflagged)
    // never appears in `batch.messages` here — guard_unsettled dropped it
    // before this call, exactly as it does for the incremental path.
    let protected = HashSet::from(["message-1".to_string()]);
    store.apply_sync_batch_protected(
        &account,
        &SyncBatch {
            messages: Vec::new(),
            replace_all_messages: true,
            cursors: vec![message_cursor("state-1", "2026-03-31T10:05:00Z")],
            ..SyncBatch::default()
        },
        &protected,
    )?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("message-1"))?
        .expect("the protected message survives");
    assert_eq!(detail.summary.mailbox_ids, vec![MailboxId::from("archive")]);
    let keywords: std::collections::BTreeSet<_> = detail.summary.keywords.iter().cloned().collect();
    assert_eq!(
        keywords,
        std::collections::BTreeSet::from(["$seen".to_string(), "$flagged".to_string()]),
    );
    Ok(())
}

#[test]
fn streamed_reconciliation_protects_unsettled_messages_from_prune() -> Result<(), StoreError> {
    // S2/P1 via the streamed path (e.g. JMAP `cannotCalculateChanges`
    // fallback): pruning happens in the final reconciliation pass, not an
    // in-batch `replace_all_messages` flag, so `reconcile_sync_protected`
    // needs the same exclusion as `apply_sync_batch_protected`.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("message-2", "inbox", Some("mime-2")),
            ],
            ..SyncBatch::default()
        },
    )?;

    // The complete remote set only contains message-2. message-1 has an
    // unsettled op and is protected; without the guard it would be pruned
    // exactly like `reconcile_prunes_local_messages_absent_from_remote_set`.
    let protected = HashSet::from(["message-1".to_string()]);
    store.reconcile_sync_protected(
        &account,
        &SyncReconciliation {
            remote_message_ids: vec![MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![message_cursor("state-final", "2026-03-31T10:05:00Z")],
        },
        &protected,
    )?;

    let messages = store.list_messages(&account, None)?;
    let ids: std::collections::BTreeSet<_> = messages.iter().map(|m| m.id.clone()).collect();
    assert_eq!(
        ids,
        std::collections::BTreeSet::from([
            MessageId::from("message-1"),
            MessageId::from("message-2"),
        ]),
        "the protected message survives reconciliation even though it is \
         absent from the complete remote set",
    );

    // The withheld cursor still commits — protection only narrows the prune.
    let cursor = store.get_cursor(&account, SyncObject::Message)?;
    assert_eq!(
        cursor.map(|cursor| cursor.state),
        Some("state-final".into())
    );
    Ok(())
}

#[test]
fn streamed_reconciliation_still_prunes_unprotected_messages() -> Result<(), StoreError> {
    // No over-protection on the streamed path either.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("message-2", "inbox", Some("mime-2")),
            ],
            ..SyncBatch::default()
        },
    )?;

    // Nothing is protected: both messages are subject to ordinary pruning,
    // and only message-2 is in the remote set.
    store.reconcile_sync_protected(
        &account,
        &SyncReconciliation {
            remote_message_ids: vec![MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![message_cursor("state-final", "2026-03-31T10:05:00Z")],
        },
        &HashSet::new(),
    )?;

    let messages = store.list_messages(&account, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, MessageId::from("message-2"));
    Ok(())
}
