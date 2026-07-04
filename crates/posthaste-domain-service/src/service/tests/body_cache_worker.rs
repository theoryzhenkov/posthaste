use super::*;

#[tokio::test]
async fn body_cache_worker_fetches_admitted_candidates_and_marks_cached() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should fetch an admitted body");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 32 * 1024);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.cached_bytes, 32 * 1024);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 0);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("message-1")]
    );
    assert_eq!(
        *store
            .applied_bodies
            .lock()
            .expect("applied bodies lock poisoned"),
        vec![(
            MessageId::from("message-1"),
            None,
            Some("Cached body".to_string())
        )]
    );
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (MessageId::from("message-1"), CacheObjectState::Cached, None),
        ]
    );
}

#[tokio::test]
async fn body_cache_worker_marks_gateway_failures_and_continues() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway =
        MutationGateway::with_fetch_body_result(Err(GatewayError::Network("offline".to_string())));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should record fetch failures");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 32 * 1024);
    assert_eq!(outcome.cached, 0);
    assert_eq!(outcome.cached_bytes, 0);
    assert_eq!(outcome.failed, 1);
    assert_eq!(outcome.skipped, 0);
    assert!(store
        .applied_bodies
        .lock()
        .expect("applied bodies lock poisoned")
        .is_empty());
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (
                MessageId::from("message-1"),
                CacheObjectState::Failed,
                Some("network_error".to_string())
            ),
        ]
    );
}

// spec: docs/eph/RFC-L2-provider-reliability (cache_maintenance arm wedge)
#[tokio::test(start_paused = true)]
async fn body_cache_worker_returns_under_its_batch_budget_when_the_source_hangs() {
    use posthaste_domain_model::BODY_CACHE_BATCH_BUDGET;

    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("message-1", 32 * 1024),
            sample_cache_fetch_candidate("message-2", 32 * 1024),
        ]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);
    // A hung body source: every fetch sleeps far past the batch budget.
    *gateway
        .fetch_body_delay
        .lock()
        .expect("delay lock poisoned") = Some(std::time::Duration::from_secs(3600));

    let started = tokio::time::Instant::now();
    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("a hung source must yield a clean partial outcome, not an error");

    // The batch RETURNED at its own deadline — it is never the thing the
    // supervisor arm budget has to drop (the drop path skips governor
    // feedback and causes the perpetual re-wedge).
    assert!(
        started.elapsed() < BODY_CACHE_BATCH_BUDGET + std::time::Duration::from_secs(5),
        "the batch must return at its own budget, elapsed {:?}",
        started.elapsed()
    );
    assert!(outcome.deadline_exceeded);
    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.cached, 0);
    assert_eq!(
        outcome.failed, 1,
        "the cut-short fetch must count as failed so the governor backs off"
    );
    // The in-flight candidate is marked Failed — never left stuck Fetching,
    // which would leak it out of the wanted set forever.
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (
                MessageId::from("message-1"),
                CacheObjectState::Failed,
                Some("batch_deadline".to_string())
            ),
        ]
    );
}

// spec: docs/eph/RFC-L2-provider-reliability (cache_maintenance arm wedge)
#[tokio::test(start_paused = true)]
async fn body_cache_worker_keeps_partial_work_from_a_slow_but_progressing_source() {
    use posthaste_domain_model::BODY_CACHE_BATCH_BUDGET;

    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("message-1", 32 * 1024),
            sample_cache_fetch_candidate("message-2", 32 * 1024),
            sample_cache_fetch_candidate("message-3", 32 * 1024),
            sample_cache_fetch_candidate("message-4", 32 * 1024),
        ]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);
    // Slow but alive: 25 s per body against a 60 s batch budget — two bodies
    // land, the third is cut at the deadline, the fourth is never started.
    *gateway
        .fetch_body_delay
        .lock()
        .expect("delay lock poisoned") = Some(std::time::Duration::from_secs(25));
    *gateway
        .fetch_body_fallback
        .lock()
        .expect("fallback lock poisoned") = Some(sample_fetched_body());

    let started = tokio::time::Instant::now();
    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("a slow source must yield a clean partial outcome");

    assert!(started.elapsed() < BODY_CACHE_BATCH_BUDGET + std::time::Duration::from_secs(5));
    assert!(outcome.deadline_exceeded);
    assert_eq!(outcome.cached, 2, "work done before the deadline is kept");
    assert_eq!(outcome.failed, 1, "the cut-short candidate is failed");
    assert_eq!(outcome.attempted, 3, "the batch stops; it does not start more");
}

#[tokio::test]
async fn body_cache_worker_surfaces_store_failures() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        apply_body_error: Some("write failed".to_string()),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let error = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect_err("cache worker should surface local store failures");

    assert_eq!(error.code(), "storage_failure");
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (
                MessageId::from("message-1"),
                CacheObjectState::Failed,
                Some("storage_failure".to_string())
            )
        ]
    );
}
