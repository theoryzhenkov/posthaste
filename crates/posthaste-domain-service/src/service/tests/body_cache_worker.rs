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
