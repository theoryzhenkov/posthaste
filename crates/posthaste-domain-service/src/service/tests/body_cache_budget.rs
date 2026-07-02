use super::*;

#[tokio::test]
async fn body_cache_worker_skips_candidates_that_do_not_fit_budget() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        cache_used_bytes: Mutex::new(1024),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        app_settings: Mutex::new(AppSettings {
            cache_policy: CachePolicy {
                soft_cap_bytes: 1024,
                hard_cap_bytes: 1024,
                cache_bodies: true,
                cache_raw_messages: false,
                cache_attachments: false,
            },
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should skip over-budget candidates");

    assert_eq!(outcome.attempted, 0);
    assert_eq!(outcome.cached, 0);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 1);
    assert!(gateway
        .fetch_attempts
        .lock()
        .expect("fetch attempts lock poisoned")
        .is_empty());
    assert!(store
        .cache_state_changes
        .lock()
        .expect("cache state changes lock poisoned")
        .is_empty());
}

#[tokio::test]
async fn body_cache_worker_scans_past_large_candidates_to_find_one_that_fits() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("too-large", 2 * 1024),
            sample_cache_fetch_candidate("small-enough", 512),
        ]),
        cache_used_bytes: Mutex::new(1024),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        app_settings: Mutex::new(AppSettings {
            cache_policy: CachePolicy {
                soft_cap_bytes: 2 * 1024,
                hard_cap_bytes: 2 * 1024,
                cache_bodies: true,
                cache_raw_messages: false,
                cache_attachments: false,
            },
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(1, 1024 * 1024))
        .await
        .expect("cache worker should scan past oversized candidates");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("small-enough")]
    );
}

#[tokio::test]
async fn body_cache_worker_respects_fetch_byte_lease() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("too-large-for-lease", 2 * 1024),
            sample_cache_fetch_candidate("fits-lease", 512),
        ]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(2, 1024))
        .await
        .expect("cache worker should respect fetch byte lease");

    assert_eq!(outcome.scanned, 2);
    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 512);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("fits-lease")]
    );
}
