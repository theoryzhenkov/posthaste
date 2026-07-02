use super::*;

#[test]
fn search_visibility_records_ranked_cache_signal_updates() {
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let page = MessagePage {
        items: vec![
            sample_message_summary("message-1", Vec::new()),
            sample_message_summary("message-2", Vec::new()),
        ],
        next_cursor: None,
    };

    let account_ids = service
        .record_cache_search_visibility(&page, 100, 2)
        .expect("visibility recording should succeed");

    assert_eq!(account_ids, vec![AccountId::from("primary")]);
    let updates = store
        .cache_signal_updates
        .lock()
        .expect("cache signal updates lock poisoned");
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].reason, "search-visible");
    assert_eq!(updates[0].search.as_ref().unwrap().total_messages, 100);
    assert_eq!(updates[0].search.as_ref().unwrap().result_count, 2);
    assert_eq!(updates[0].search.as_ref().unwrap().result_rank, 0);
    assert_eq!(updates[1].search.as_ref().unwrap().result_rank, 1);
    assert!(updates[0].direct_user_boost.unwrap() > updates[1].direct_user_boost.unwrap());
}

#[test]
fn cache_rescore_batch_applies_search_signal_priority() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_rescore_candidates: Mutex::new(vec![sample_cache_rescore_candidate("message-1")]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let outcome = service
        .process_cache_rescore_batch(&account_id, 10)
        .expect("rescore should succeed");

    assert_eq!(outcome.scanned, 1);
    assert_eq!(outcome.updated, 1);
    let updates = store
        .cache_priority_updates
        .lock()
        .expect("cache priority updates lock poisoned");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].message_id, "message-1");
    assert_eq!(updates[0].reason, "search-visible");
    assert!(updates[0].priority > 1.0);
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn cache_rescore_batch_rebuilds_imap_body_fetch_cost_from_metadata() {
    let mut account = sample_source();
    account.driver = AccountDriver::ImapSmtp;
    let account_id = account.id.clone();
    let mut candidate = sample_cache_rescore_candidate("message-1");
    candidate.fetch_unit = CacheFetchUnit::BodyOnly;
    candidate.value_bytes = 0;
    candidate.fetch_bytes = 0;
    candidate.message_size = 12 * 1024 * 1024;
    candidate.has_attachment = true;
    let store = Arc::new(TestStore {
        cache_rescore_candidates: Mutex::new(vec![candidate]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let outcome = service
        .process_cache_rescore_batch(&account_id, 10)
        .expect("rescore should succeed");

    assert_eq!(outcome.updated, 1);
    let updates = store
        .cache_priority_updates
        .lock()
        .expect("cache priority updates lock poisoned");
    assert_eq!(updates[0].fetch_unit, CacheFetchUnit::RawMessage);
    assert_eq!(updates[0].value_bytes, 256 * 1024);
    assert_eq!(updates[0].fetch_bytes, 12 * 1024 * 1024);
}

// spec: docs/L1-sync#cache-stale-rescore
#[test]
fn stale_cache_rescore_batch_queues_bounded_cutoff() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        stale_cache_rescore_result: 7,
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);

    let queued = service
        .queue_stale_cache_rescore_batch(&account_id, Duration::from_secs(60), 25)
        .expect("stale queue should succeed");

    assert_eq!(queued, 7);
    let requests = store
        .stale_cache_rescore_requests
        .lock()
        .expect("stale cache rescore requests lock poisoned");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, account_id);
    assert_eq!(requests[0].2, 25);
    assert!(!requests[0].1.is_empty());
}
