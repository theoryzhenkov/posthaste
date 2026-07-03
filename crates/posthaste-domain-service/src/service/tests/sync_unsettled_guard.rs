use super::*;

/// Queue a `SetKeywords` op for `message-1` and arm the gateway so the flush
/// inside `sync_account_with_mode` fails transiently — the op resets to
/// `Pending` (not settled, not removed), so `message-1` stays in
/// `unsettled_message_ids` for the OBSERVE phase that follows. This is the M35
/// durable-guard scenario: a locally-modified message the sync must fold over,
/// not clobber or prune.
async fn queue_unsettled_message(service: &MailService, account_id: &AccountId) {
    service
        .set_keywords(
            account_id,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("set_keywords queues an op");
}

/// The keywords of `message_id` in the last batch the store applied, if any.
fn last_applied_keywords(store: &TestStore, message_id: &str) -> Option<Vec<String>> {
    store
        .applied_messages
        .lock()
        .expect("applied messages lock poisoned")
        .iter()
        .rev()
        .find(|record| record.id.as_str() == message_id)
        .map(|record| record.keywords.clone())
}

#[tokio::test]
async fn full_snapshot_folds_a_pending_flag_over_server_truth() {
    // THE M35 HEADLINE (D93): a full snapshot (replace_all — the shape an
    // initial sync / UIDVALIDITY resync / QRESYNC resync takes) carries
    // message-1 as *server* truth (unflagged). A local `$flagged` that hasn't
    // round-tripped must NOT revert; the durable guard folds it back over the
    // snapshot row rather than dropping the row, so server truth still lands and
    // the un-acked flag rides on top.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    queue_unsettled_message(&service, &account_id).await;

    // The server snapshot's message-1 is unflagged (keywords default-empty).
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![sample_message_record("message-1", 1024, false)],
            replace_all_messages: true,
            ..SyncBatch::default()
        },
    );
    // Both in-cycle flushes fail transiently so message-1 stays un-acked.
    gateway
        .set_keywords_results
        .lock()
        .expect("set keywords results lock poisoned")
        .extend([
            Err(GatewayError::Network("offline".to_string())),
            Err(GatewayError::Network("offline".to_string())),
        ]);

    let mut publish = |_: &[DomainEvent]| {};
    service
        .sync_account_with_mode(
            &account_id,
            SyncTrigger::Manual,
            SyncMode::Incremental,
            &gateway,
            None,
            &mut publish,
        )
        .await
        .expect("sync completes");

    // The row the snapshot upserted for message-1 carries the un-acked flag —
    // folded over server truth, not reverted.
    let keywords = last_applied_keywords(&store, "message-1")
        .expect("the snapshot still upserts message-1 (folded, not dropped)");
    assert!(
        keywords.iter().any(|keyword| keyword == "$flagged"),
        "the un-acked $flagged survives the full snapshot (folded over server truth), got {keywords:?}",
    );
}

#[tokio::test]
async fn an_acked_operation_is_superseded_by_the_snapshot() {
    // The ack gate (M32 outbox settlement): once the pre-observe FLUSH acks the
    // op, message-1 leaves the unsettled set, so the snapshot's server truth
    // supersedes it cleanly — no stale overlay re-layered, and message-1 is NOT
    // in the store's protected set.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    queue_unsettled_message(&service, &account_id).await;

    // No pushed set_keywords errors: the first flush settles (acks) the op.
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![sample_message_record("message-1", 1024, false)],
            replace_all_messages: true,
            ..SyncBatch::default()
        },
    );

    let mut publish = |_: &[DomainEvent]| {};
    service
        .sync_account_with_mode(
            &account_id,
            SyncTrigger::Manual,
            SyncMode::Incremental,
            &gateway,
            None,
            &mut publish,
        )
        .await
        .expect("sync completes");

    assert!(
        service
            .unsettled_message_ids(&account_id)
            .expect("unsettled set")
            .is_empty(),
        "the op acked on the pre-observe flush, so nothing stays unsettled",
    );
    // Server truth won: the upserted row has no folded-in $flagged.
    let keywords = last_applied_keywords(&store, "message-1")
        .expect("the snapshot upserts message-1 with plain server truth");
    assert!(
        !keywords.iter().any(|keyword| keyword == "$flagged"),
        "an acked op is not re-layered — the snapshot supersedes it, got {keywords:?}",
    );
    let protected_calls = store
        .protected_message_ids
        .lock()
        .expect("protected message ids lock poisoned");
    assert!(
        protected_calls
            .iter()
            .all(|protected| !protected.contains("message-1")),
        "an acked message is not protected — the snapshot owns it",
    );
}

#[tokio::test]
async fn full_resync_preserves_pending_intent_via_stable_message_identity() {
    // D95: a UIDVALIDITY change (or any full resync) invalidates local UID
    // mappings and arrives as a `replace_all` snapshot under *fresh* provider
    // UIDs. Pending user intent must survive the remap. The guard keys on the
    // stable MessageId (Gmail X-GM-MSGID, or the store's identity), not the UID,
    // so as long as the snapshot carries message-1 under its stable id the
    // pending `$flagged` is folded back — no revert, and exactly one row (no
    // duplicate protected-old + inserted-new).
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    queue_unsettled_message(&service, &account_id).await;

    // The post-UIDVALIDITY full snapshot re-lists message-1 under its stable id.
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![sample_message_record("message-1", 1024, false)],
            replace_all_messages: true,
            ..SyncBatch::default()
        },
    );
    gateway
        .set_keywords_results
        .lock()
        .expect("set keywords results lock poisoned")
        .extend([
            Err(GatewayError::Network("offline".to_string())),
            Err(GatewayError::Network("offline".to_string())),
        ]);

    let mut publish = |_: &[DomainEvent]| {};
    service
        .sync_account_with_mode(
            &account_id,
            SyncTrigger::Manual,
            SyncMode::Incremental,
            &gateway,
            None,
            &mut publish,
        )
        .await
        .expect("resync completes");

    let applied = store
        .applied_messages
        .lock()
        .expect("applied messages lock poisoned");
    let message_1_rows = applied
        .iter()
        .filter(|record| record.id.as_str() == "message-1")
        .count();
    assert_eq!(
        message_1_rows, 1,
        "message-1 is upserted exactly once across the resync — folded, not duplicated",
    );
    let keywords = applied
        .iter()
        .rev()
        .find(|record| record.id.as_str() == "message-1")
        .map(|record| record.keywords.clone())
        .expect("message-1 present under its stable id");
    assert!(
        keywords.iter().any(|keyword| keyword == "$flagged"),
        "pending intent survives the UIDVALIDITY remap via stable identity, got {keywords:?}",
    );
}

#[tokio::test]
async fn replace_all_snapshot_routes_through_the_protected_apply_with_the_unsettled_set() {
    // P1/S2: a full IMAP resync (or any `replace_all_messages` batch) must not
    // silently clobber-or-prune an in-flight local message. Verifies the
    // service-level wiring: the store boundary receives `apply_sync_batch_protected`
    // (not the unguarded `apply_sync_batch`) with message-1 in the protected set.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    queue_unsettled_message(&service, &account_id).await;

    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![sample_message_record("message-2", 1024, false)],
            replace_all_messages: true,
            ..SyncBatch::default()
        },
    );
    // `sync_account_with_mode` flushes twice — once before OBSERVE, once after
    // (so a queued automation op rides the same cycle). Fail both transiently
    // so message-1's op stays Pending (unsettled) through the whole call,
    // including the OBSERVE phase this test is targeting.
    gateway
        .set_keywords_results
        .lock()
        .expect("set keywords results lock poisoned")
        .extend([
            Err(GatewayError::Network("offline".to_string())),
            Err(GatewayError::Network("offline".to_string())),
        ]);

    let mut publish = |_: &[DomainEvent]| {};
    service
        .sync_account_with_mode(
            &account_id,
            SyncTrigger::Manual,
            SyncMode::Incremental,
            &gateway,
            None,
            &mut publish,
        )
        .await
        .expect("sync completes despite the transient flush failure");

    assert!(
        service
            .unsettled_message_ids(&account_id)
            .expect("unsettled set")
            .contains("message-1"),
        "the transiently-failed op is still unsettled going into OBSERVE",
    );

    let protected_calls = store
        .protected_message_ids
        .lock()
        .expect("protected message ids lock poisoned");
    assert!(
        protected_calls
            .iter()
            .any(|protected| protected.contains("message-1")),
        "the replace_all batch must reach the store via apply_sync_batch_protected \
         with message-1 in the protected set, not the unguarded apply_sync_batch",
    );
}

#[tokio::test]
async fn streamed_full_snapshot_reconciliation_routes_through_the_protected_reconcile() {
    // The JMAP `cannotCalculateChanges` fallback (and any other streamed
    // upsert-only full snapshot) prunes in the *reconciliation* pass, not the
    // per-chunk batch — this must also see the unsettled set, via
    // `reconcile_sync_protected` rather than the unguarded `reconcile_sync`.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    queue_unsettled_message(&service, &account_id).await;

    let chunk = SyncBatch {
        messages: vec![sample_message_record("message-2", 1024, false)],
        ..SyncBatch::default()
    };
    let gateway = MutationGateway::with_stream(
        vec![chunk],
        posthaste_domain_model::SyncReconciliation {
            // message-1 is not in the complete remote set — a plain prune
            // pass would delete it; the protected set must save it.
            remote_message_ids: vec![MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "final-state".to_string(),
                updated_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
            }],
        },
    );
    // See the comment on the previous test: two flushes happen inside one
    // `sync_account_with_mode` call, so both must fail transiently for
    // message-1 to stay unsettled through the reconciliation pass.
    gateway
        .set_keywords_results
        .lock()
        .expect("set keywords results lock poisoned")
        .extend([
            Err(GatewayError::Network("offline".to_string())),
            Err(GatewayError::Network("offline".to_string())),
        ]);

    let mut publish = |_: &[DomainEvent]| {};
    service
        .sync_account_with_mode(
            &account_id,
            SyncTrigger::Manual,
            SyncMode::Incremental,
            &gateway,
            None,
            &mut publish,
        )
        .await
        .expect("streamed sync completes despite the transient flush failure");

    assert!(
        service
            .unsettled_message_ids(&account_id)
            .expect("unsettled set")
            .contains("message-1"),
        "the transiently-failed op is still unsettled going into the reconciliation pass",
    );

    let state = store
        .mutation_state
        .lock()
        .expect("mutation state lock poisoned");
    assert_eq!(state.reconcile_calls, 1, "reconciliation ran exactly once");
    drop(state);

    let protected_calls = store
        .protected_message_ids
        .lock()
        .expect("protected message ids lock poisoned");
    assert!(
        protected_calls
            .iter()
            .any(|protected| protected.contains("message-1")),
        "the final reconciliation pass must reach the store via \
         reconcile_sync_protected with message-1 in the protected set",
    );
}
