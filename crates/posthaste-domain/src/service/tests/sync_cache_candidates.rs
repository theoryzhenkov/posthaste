use super::*;

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
