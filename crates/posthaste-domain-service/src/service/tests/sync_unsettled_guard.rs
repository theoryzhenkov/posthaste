use super::*;

/// Queue a `SetKeywords` op for `message-1` and arm the gateway so the flush
/// inside `sync_account_with_mode` fails transiently — the op resets to
/// `Pending` (not settled, not removed), so `message-1` stays in
/// `unsettled_message_ids` for the OBSERVE phase that follows. This is the S3
/// unsettled-guard scenario: a locally-modified message the sync must not
/// clobber or prune.
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
