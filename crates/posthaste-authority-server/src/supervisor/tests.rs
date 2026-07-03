use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountTransportSettings, EventFilter, ProviderAuthKind, ProviderHint, PushNotification,
    SecretRef, SecretStoreError, RFC3339_EPOCH,
};
use posthaste_store::DatabaseStore;

use super::*;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestSecretStore;

impl SecretStore for TestSecretStore {
    fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        Err(SecretStoreError::Unavailable("unused".to_string()))
    }

    fn save(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }

    fn update(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }

    fn delete(&self, _secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
}

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-supervisor-test-{now}-{seq}"))
}

fn test_account(id: &str) -> AccountSettings {
    AccountSettings {
        id: AccountId::from(id),
        name: "Test".to_string(),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: RFC3339_EPOCH.to_string(),
        updated_at: RFC3339_EPOCH.to_string(),
    }
}

fn test_shared(account: &AccountSettings) -> Arc<SupervisorShared> {
    let root = temp_root();
    let config_root = root.join("config");
    let state_root = root.join("state");
    let config_repo =
        TomlConfigRepository::open(&config_root).expect("config repository should open");
    config_repo
        .initialize_defaults()
        .expect("config defaults should initialize");
    let store = Arc::new(
        DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
            .expect("database store should open"),
    );
    let config = Arc::new(config_repo);
    let service = Arc::new(MailService::new(store.clone(), config));
    service
        .save_source(account)
        .expect("test account should save");
    let (event_sender, _) = broadcast::channel(16);

    Arc::new(SupervisorShared {
        service,
        store,
        secret_store: Arc::new(TestSecretStore),
        event_sender,
        gateways: RwLock::new(HashMap::new()),
        runtime_overviews: RwLock::new(HashMap::new()),
        runtime_generations: RwLock::new(HashMap::new()),
        known_accounts: RwLock::new(HashSet::new()),
        account_count: AtomicUsize::new(0),
        cache_resources: Mutex::new(CacheResourceGovernor::new(
            Instant::now(),
            CacheResourcePolicy::default(),
        )),
        poll_interval: Duration::from_secs(60),
    })
}

#[test]
fn runtime_connection_state_tracks_connected_gateway() {
    let gateway: SharedGateway = Arc::new(MockJmapGateway::default());
    let mut state = AccountRuntimeConnectionState::default();

    assert!(!state.is_connected());
    assert!(state.gateway().is_none());

    state.set_connected(AccountConnection {
        gateway: gateway.clone(),
        push_events: None,
        remote_observation: RemoteObservationPolicy::disabled(),
        secret_resolver: Arc::new(StaticSecretResolver::new("")),
    });

    assert!(state.is_connected());
    assert!(Arc::ptr_eq(&state.gateway().expect("gateway"), &gateway));

    state.disconnect();

    assert!(!state.is_connected());
    assert!(state.gateway().is_none());
}

#[test]
fn sync_failure_stage_classifies_connection_failures() {
    let unavailable: ServiceError = GatewayError::Unavailable("account".to_string()).into();
    let auth: ServiceError = GatewayError::Auth.into();
    let network: ServiceError = GatewayError::Network("offline".to_string()).into();

    assert_eq!(sync_failure_stage(&unavailable), "connect");
    assert_eq!(sync_failure_stage(&auth), "connect");
    assert_eq!(sync_failure_stage(&network), "connect");
}

#[test]
fn sync_failure_stage_classifies_non_connection_failures_as_sync() {
    let rejected: ServiceError = GatewayError::Rejected("bad request".to_string()).into();
    let state_mismatch: ServiceError = GatewayError::StateMismatch.into();

    assert_eq!(sync_failure_stage(&rejected), "sync");
    assert_eq!(sync_failure_stage(&state_mismatch), "sync");
}

// spec: docs/L1-sync#event-propagation
#[tokio::test]
async fn stale_runtime_generation_cannot_overwrite_current_runtime_status() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let stale_generation = shared.next_runtime_generation(&account.id).await;
    let current_generation = shared.next_runtime_generation(&account.id).await;

    shared
        .set_runtime_overview_for_generation(
            &account.id,
            current_generation,
            AccountRuntimeOverview {
                status: AccountStatus::Ready,
                push: PushStatus::Connected,
                ..Default::default()
            },
        )
        .await;
    shared
        .set_runtime_overview_for_generation(
            &account.id,
            stale_generation,
            AccountRuntimeOverview {
                status: AccountStatus::Offline,
                push: PushStatus::Reconnecting,
                last_sync_error: Some("stale".to_string()),
                ..Default::default()
            },
        )
        .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(overview.status, AccountStatus::Ready);
    assert_eq!(overview.push, PushStatus::Connected);
    assert_eq!(overview.last_sync_error, None);
}

// spec: docs/runtime/adapter/L2#account-status-views
#[tokio::test]
async fn supervisor_account_count_tracks_known_accounts() {
    let account = test_account("primary");
    let shared = test_shared(&account);

    assert_eq!(shared.account_count.load(Ordering::SeqCst), 0);
    shared.register_account(&account.id).await;
    assert_eq!(shared.account_count.load(Ordering::SeqCst), 1);
    shared.register_account(&account.id).await;
    assert_eq!(shared.account_count.load(Ordering::SeqCst), 1);
    shared.unregister_account(&account.id).await;
    assert_eq!(shared.account_count.load(Ordering::SeqCst), 0);
}

// spec: docs/L1-sync#event-propagation
#[tokio::test]
async fn push_only_runtime_transition_emits_account_status_changed() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;

    shared
        .set_runtime_overview_for_generation(
            &account.id,
            generation,
            AccountRuntimeOverview {
                status: AccountStatus::Ready,
                push: PushStatus::Reconnecting,
                ..Default::default()
            },
        )
        .await;
    shared
        .set_runtime_overview_for_generation(
            &account.id,
            generation,
            AccountRuntimeOverview {
                status: AccountStatus::Ready,
                push: PushStatus::Connected,
                ..Default::default()
            },
        )
        .await;

    let events = shared
        .store
        .list_events(&EventFilter {
            account_id: Some(account.id.clone()),
            topic: Some(EVENT_TOPIC_ACCOUNT_STATUS_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })
        .expect("events should list");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].payload["push"], "connected");
}

fn sync_progress(stage: SyncProgressStage) -> SyncProgress {
    SyncProgress {
        sync_id: "sync-1".to_string(),
        trigger: SyncTrigger::Manual,
        started_at: "2026-04-29T00:00:00Z".to_string(),
        stage,
        detail: String::new(),
        mailbox_name: None,
        mailbox_index: None,
        mailbox_count: None,
        message_count: None,
        total_count: None,
    }
}

// spec: docs/L1-sync#event-propagation
#[tokio::test]
async fn late_sync_progress_does_not_revive_syncing_after_success() {
    // Regression for the stuck-"syncing" bug: a sync-progress write that lands
    // after the sync settled must not revive `Syncing` over the terminal
    // `Ready`. The guard runs against committed state under the lock, so the
    // late Storing write is dropped.
    let account = test_account("primary");
    let shared = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;

    shared
        .set_sync_progress(
            &account.id,
            generation,
            Some(sync_progress(SyncProgressStage::Connecting)),
        )
        .await;
    shared.mark_sync_success(&account.id, generation).await;
    // A late reporter write arriving after success:
    shared
        .set_sync_progress(
            &account.id,
            generation,
            Some(sync_progress(SyncProgressStage::Storing)),
        )
        .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(overview.status, AccountStatus::Ready);
    assert!(overview.sync_progress.is_none());

    let events = shared
        .store
        .list_events(&EventFilter {
            account_id: Some(account.id.clone()),
            topic: Some(EVENT_TOPIC_ACCOUNT_STATUS_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })
        .expect("events should list");
    assert_eq!(
        events.last().expect("a status event").payload["status"],
        "ready",
        "the last delivered status must be the terminal ready, not a revived syncing",
    );
}

// spec: docs/L1-sync#event-propagation
#[tokio::test]
async fn concurrent_progress_writes_cannot_clobber_sync_success() {
    // Many sync-progress writes racing the sync-success write must always
    // settle on `Ready`: the atomic read-modify-write means a write that runs
    // before success is overwritten by it, and one that runs after is dropped
    // by the guard. The previous non-atomic RMW could let a stale-read progress
    // write clobber `Ready` with `Syncing`.
    let account = test_account("primary");
    let shared = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    shared
        .set_sync_progress(
            &account.id,
            generation,
            Some(sync_progress(SyncProgressStage::Connecting)),
        )
        .await;

    let mut handles = Vec::new();
    for _ in 0..50 {
        let shared = shared.clone();
        let account_id = account.id.clone();
        handles.push(tokio::spawn(async move {
            shared
                .set_sync_progress(
                    &account_id,
                    generation,
                    Some(sync_progress(SyncProgressStage::Storing)),
                )
                .await;
        }));
    }
    shared.mark_sync_success(&account.id, generation).await;
    for handle in handles {
        handle.await.expect("progress task should not panic");
    }

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(
        overview.status,
        AccountStatus::Ready,
        "no concurrent progress write may revive syncing after success",
    );
    assert!(overview.sync_progress.is_none());
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn checkpoint_only_push_notification_triggers_sync() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let gateway: SharedGateway = Arc::new(MockJmapGateway::default());
    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway,
        push_events: None,
        remote_observation: account
            .transport
            .provider_profile()
            .jmap()
            .remote_observation(),
        secret_resolver: Arc::new(StaticSecretResolver::new("")),
    });
    let notification = PushNotification {
        account_id: account.id.clone(),
        changed: Vec::new(),
        received_at: "2026-04-29T00:00:00Z".to_string(),
        checkpoint: Some("event-42".to_string()),
    };

    let sync_state = SyncTriggerState::new();
    let generation = shared.next_runtime_generation(&account.id).await;
    let triggered = handle_push_event(
        &sync_state,
        &shared,
        &account,
        &account.id,
        generation,
        &mut connection,
        PushStreamEvent::Notification(notification),
    )
    .await;

    assert!(triggered);
    assert!(!shared
        .service
        .list_messages(&account.id, None)
        .expect("messages should list")
        .is_empty());
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn gmail_imap_idle_hint_without_changed_ids_triggers_full_observation_sync() {
    let mut account = test_account("gmail");
    account.driver = AccountDriver::ImapSmtp;
    account.transport.provider = ProviderHint::Gmail;
    let shared = test_shared(&account);
    let gateway: SharedGateway = Arc::new(MockJmapGateway::default());
    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway,
        push_events: None,
        remote_observation: account
            .transport
            .provider_profile()
            .imap()
            .remote_observation(),
        secret_resolver: Arc::new(StaticSecretResolver::new("")),
    });
    let notification = PushNotification {
        account_id: account.id.clone(),
        changed: Vec::new(),
        received_at: "2026-04-29T00:00:00Z".to_string(),
        checkpoint: None,
    };

    let sync_state = SyncTriggerState::new();
    let generation = shared.next_runtime_generation(&account.id).await;
    let triggered = handle_push_event(
        &sync_state,
        &shared,
        &account,
        &account.id,
        generation,
        &mut connection,
        PushStreamEvent::Notification(notification),
    )
    .await;

    assert!(triggered);
    assert!(account
        .transport
        .provider_profile()
        .imap()
        .remote_observation()
        .treats_hints_as_incomplete());
    assert!(!shared
        .service
        .list_messages(&account.id, None)
        .expect("messages should list")
        .is_empty());
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn jmap_empty_push_notification_without_checkpoint_is_ignored() {
    let mut account = test_account("jmap");
    account.driver = AccountDriver::Jmap;
    let shared = test_shared(&account);
    let gateway: SharedGateway = Arc::new(MockJmapGateway::default());
    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway,
        push_events: None,
        remote_observation: account
            .transport
            .provider_profile()
            .jmap()
            .remote_observation(),
        secret_resolver: Arc::new(StaticSecretResolver::new("")),
    });
    let notification = PushNotification {
        account_id: account.id.clone(),
        changed: Vec::new(),
        received_at: "2026-04-29T00:00:00Z".to_string(),
        checkpoint: None,
    };

    let sync_state = SyncTriggerState::new();
    let generation = shared.next_runtime_generation(&account.id).await;
    let triggered = handle_push_event(
        &sync_state,
        &shared,
        &account,
        &account.id,
        generation,
        &mut connection,
        PushStreamEvent::Notification(notification),
    )
    .await;

    assert!(!triggered);
    assert!(shared
        .service
        .list_messages(&account.id, None)
        .expect("messages should list")
        .is_empty());
}

#[tokio::test]
async fn oauth_refresh_state_is_enabled_for_oauth_account() {
    let mut account = test_account("oauth");
    account.transport.auth = ProviderAuthKind::OAuth2;
    let mut state = OAuthRefreshState::new(&account);

    assert!(state.enabled());
    assert!(state.interval().is_some());
}

#[test]
fn oauth_refresh_state_is_disabled_for_non_oauth_account() {
    let account = test_account("basic");
    let mut state = OAuthRefreshState::new(&account);

    assert!(!state.enabled());
    assert!(state.interval().is_none());
}

// ---------------------------------------------------------------------------
// M21: cooperative stop + panic-surfacing watchdog (RFC-L2-lifecycle D61).
// ---------------------------------------------------------------------------

/// Sets a flag on drop, so an aborted incarnation task can be proven to have
/// actually been dropped (i.e. the stop escalation fired), not merely detached.
struct AbortFlag(Arc<AtomicBool>);
impl Drop for AbortFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A fast, jitter-free watchdog policy for the panic/halt tests (the storm test
/// pins the real backoff instead).
fn fast_watchdog_policy(max_restarts: u32) -> WatchdogPolicy {
    WatchdogPolicy {
        max_restarts,
        healthy_reset_after: Duration::from_secs(60),
        backoff: BackoffPolicy {
            base: Duration::from_millis(1),
            factor: 2.0,
            cap: Duration::from_millis(5),
        },
        jitter: Arc::new(|| 0.0),
    }
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d61
#[tokio::test]
async fn stop_all_joins_cooperative_accounts_and_escalates_a_straggler() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let supervisor = AccountSupervisor::from_shared_for_test(shared);

    // A cooperative account: its incarnation exits promptly on cancel.
    supervisor
        .spawn_supervised_for_test(
            AccountId::from("cooperative"),
            WatchdogPolicy::production(),
            Arc::new(|cancel: CancellationToken| tokio::spawn(async move { cancel.cancelled().await })),
        )
        .await;

    // A straggler that ignores cancellation entirely — only the abort escalation
    // can stop it; its Drop guard flips a flag so we can prove that happened.
    let straggler_aborted = Arc::new(AtomicBool::new(false));
    let flag = straggler_aborted.clone();
    supervisor
        .spawn_supervised_for_test(
            AccountId::from("straggler"),
            WatchdogPolicy::production(),
            Arc::new(move |_cancel: CancellationToken| {
                let flag = flag.clone();
                tokio::spawn(async move {
                    let _guard = AbortFlag(flag);
                    std::future::pending::<()>().await;
                })
            }),
        )
        .await;

    let start = tokio::time::Instant::now();
    tokio::time::timeout(
        Duration::from_secs(5),
        supervisor.stop_all(Duration::from_millis(200)),
    )
    .await
    .expect("stop_all must return within the test bound, not hang on the straggler");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "stop_all must not block on the straggler past its deadline (took {elapsed:?})"
    );
    assert!(
        supervisor.runtimes.read().await.is_empty(),
        "stop_all must drain every account from the registry"
    );

    // The straggler must have been aborted (dropped) by the escalation.
    let mut aborted = false;
    for _ in 0..200 {
        if straggler_aborted.load(Ordering::SeqCst) {
            aborted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        aborted,
        "the straggler incarnation must be aborted (escalation), not left running"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d61
#[tokio::test]
async fn watchdog_surfaces_panic_and_restarts_with_degraded_status() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let account_id = account.id.clone();

    // First incarnation panics; the second stays healthy until cancelled.
    let spawns = Arc::new(AtomicUsize::new(0));
    let counter = spawns.clone();
    let spawn: SpawnIncarnation = Box::new(move |cancel: CancellationToken| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            assert!(n != 0, "boom on first incarnation");
            cancel.cancelled().await;
        })
    });

    let cancel = CancellationToken::new();
    let first = spawn(cancel.clone());
    let monitor = tokio::spawn(run_watchdog(
        account_id.clone(),
        shared.clone(),
        cancel.clone(),
        spawn,
        fast_watchdog_policy(3),
        first,
    ));

    // The panic is surfaced as a Degraded status and the account is restarted.
    let mut recovered = false;
    for _ in 0..400 {
        let overview = shared.runtime_overview(&account_id).await;
        if spawns.load(Ordering::SeqCst) >= 2 && overview.status == AccountStatus::Degraded {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        recovered,
        "a panicked account must surface a Degraded status (not silence) and restart"
    );
    let overview = shared.runtime_overview(&account_id).await;
    assert_eq!(
        overview.last_sync_error_code.as_deref(),
        Some("runtime_fault"),
        "the truthful error code for a panicked-but-retried account"
    );

    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), monitor).await;
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d61
#[tokio::test]
async fn watchdog_halts_account_with_offline_status_after_restart_cap() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let account_id = account.id.clone();

    // Every incarnation panics — the watchdog must give up at the cap.
    let spawns = Arc::new(AtomicUsize::new(0));
    let counter = spawns.clone();
    let spawn: SpawnIncarnation = Box::new(move |_cancel| {
        counter.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async {
            panic!("always panics");
        })
    });

    let cancel = CancellationToken::new();
    let first = spawn(cancel.clone());
    tokio::time::timeout(
        Duration::from_secs(5),
        run_watchdog(
            account_id.clone(),
            shared.clone(),
            cancel,
            spawn,
            fast_watchdog_policy(WATCHDOG_MAX_RESTARTS),
            first,
        ),
    )
    .await
    .expect("the watchdog must halt (return) after exhausting the restart cap");

    assert_eq!(
        spawns.load(Ordering::SeqCst),
        1 + WATCHDOG_MAX_RESTARTS as usize,
        "one original run plus exactly the capped number of restart attempts"
    );
    let overview = shared.runtime_overview(&account_id).await;
    assert_eq!(
        overview.status,
        AccountStatus::Offline,
        "a halted account is truthfully Offline"
    );
    assert_eq!(
        overview.last_sync_error_code.as_deref(),
        Some("runtime_halted")
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d61
#[tokio::test(start_paused = true)]
async fn watchdog_backoff_prevents_restart_storm() {
    let account = test_account("primary");
    let shared = test_shared(&account);
    let account_id = account.id.clone();

    // Record the virtual-clock instant of every (re)spawn.
    let spawn_times = Arc::new(StdMutex::new(Vec::<tokio::time::Instant>::new()));
    let times = spawn_times.clone();
    let spawn: SpawnIncarnation = Box::new(move |_cancel| {
        times
            .lock()
            .expect("times mutex")
            .push(tokio::time::Instant::now());
        tokio::spawn(async {
            panic!("panic to force backoff");
        })
    });

    // The M9 near-end engine's backoff shape, worst-case (full) jitter.
    let backoff = BackoffPolicy {
        base: Duration::from_millis(500),
        factor: 2.0,
        cap: Duration::from_secs(30),
    };
    let policy = WatchdogPolicy {
        max_restarts: 3,
        healthy_reset_after: Duration::from_secs(3600),
        backoff: backoff.clone(),
        jitter: Arc::new(|| 1.0),
    };

    let cancel = CancellationToken::new();
    let first = spawn(cancel.clone());
    run_watchdog(account_id, shared, cancel, spawn, policy, first).await;

    let times = spawn_times.lock().expect("times mutex").clone();
    assert_eq!(times.len(), 4, "1 original run + 3 capped restarts before halt");
    let gaps: Vec<Duration> = times
        .windows(2)
        .map(|pair| pair[1].duration_since(pair[0]))
        .collect();
    // Each restart waits at least the backoff ceiling for that attempt — the
    // restarts are spaced by exponential backoff, never a tight storm.
    assert!(gaps[0] >= backoff.ceiling(0), "attempt 1 waits >= base");
    assert!(gaps[1] >= backoff.ceiling(1), "attempt 2 waits >= 2x base");
    assert!(gaps[2] >= backoff.ceiling(2), "attempt 3 waits >= 4x base");
}
