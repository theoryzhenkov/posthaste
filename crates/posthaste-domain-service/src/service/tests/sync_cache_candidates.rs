use super::*;

#[tokio::test]
async fn streamed_sync_applies_chunks_progressively_then_reconciles() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let chunk = |id: &str| SyncBatch {
        messages: vec![sample_message_record(id, 1024, false)],
        ..SyncBatch::default()
    };
    let gateway = MutationGateway::with_stream(
        vec![chunk("message-1"), chunk("message-2")],
        crate::SyncReconciliation {
            remote_message_ids: vec![MessageId::from("message-1"), MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "final-state".to_string(),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }],
        },
    );

    let mut published_groups = 0usize;
    let mut publish = |_: &[DomainEvent]| published_groups += 1;
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
        .expect("streamed sync should succeed");

    let state = store
        .mutation_state
        .lock()
        .expect("mutation state lock poisoned");
    // Each chunk is applied in its own batch as it arrives, not accumulated.
    assert_eq!(state.applied_message_chunks, vec![1, 1]);
    // The reconciliation pass runs once and commits the withheld cursor.
    assert_eq!(state.reconcile_calls, 1);
    assert_eq!(
        state.cursor.as_ref().map(|cursor| cursor.state.as_str()),
        Some("final-state")
    );
    // Events were broadcast as produced (per chunk + reconcile + tail groups),
    // not buffered to the end.
    assert!(published_groups > 2);
}

#[tokio::test]
async fn sync_account_records_body_cache_candidate_with_body_only_fetch_cost() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![sample_message_record("message-1", 12 * 1024 * 1024, true)],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should succeed");

    let candidates = store
        .cache_candidates
        .lock()
        .expect("cache candidates lock poisoned");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].layer, CacheLayer::Body);
    assert_eq!(candidates[0].fetch_unit, CacheFetchUnit::BodyOnly);
    assert_eq!(candidates[0].fetch_bytes, 256 * 1024);
}

#[tokio::test]
async fn sync_account_records_imap_body_candidate_with_raw_message_fetch_cost() {
    let mut account = sample_source();
    account.driver = AccountDriver::ImapSmtp;
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![sample_message_record("message-1", 12 * 1024 * 1024, true)],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should succeed");

    let candidates = store
        .cache_candidates
        .lock()
        .expect("cache candidates lock poisoned");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].layer, CacheLayer::Body);
    assert_eq!(candidates[0].fetch_unit, CacheFetchUnit::RawMessage);
    assert_eq!(candidates[0].value_bytes, 256 * 1024);
    assert_eq!(candidates[0].fetch_bytes, 12 * 1024 * 1024);
}
