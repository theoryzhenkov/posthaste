use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountTransportSettings, EventFilter, ProviderAuthKind, ProviderHint, PushNotification,
    SecretRef, SecretStoreError, RFC3339_EPOCH,
};
use posthaste_store::DatabaseStore;

use crate::test_support::{temp_root, TempDirGuard};

use super::*;

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

// Returns the `TempDirGuard` alongside the shared state: both the sqlite
// mail store and the TOML config repository re-open files under this
// directory on every call, so the guard must outlive the caller's use of the
// returned `SupervisorShared`, not just this function body (P6 — dropping it
// here would delete the directory out from under still-live config I/O).
fn test_shared(account: &AccountSettings) -> (Arc<SupervisorShared>, TempDirGuard) {
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

    let shared = Arc::new(SupervisorShared {
        service,
        store,
        secret_store: Arc::new(TestSecretStore),
        event_sender,
        gateways: RwLock::new(HashMap::new()),
        runtime_overviews: RwLock::new(HashMap::new()),
        runtime_generations: RwLock::new(HashMap::new()),
        sync_cycle_generations: RwLock::new(HashMap::new()),
        known_accounts: RwLock::new(HashSet::new()),
        account_count: AtomicUsize::new(0),
        cache_resources: Mutex::new(CacheResourceGovernor::new(
            Instant::now(),
            CacheResourcePolicy::default(),
        )),
        sync_governor: SyncGovernor::production(),
        poll_interval: Duration::from_secs(60),
        oauth_refresh_flights: Mutex::new(HashMap::new()),
    });
    (shared, root)
}

// A shared context with an explicit scheduling governor (D98) — used by the
// boot-storm test to pin a tiny concurrent-sync cap and zero startup splay so
// the global cap (not the splay) is what bounds the observed peak.
fn test_shared_with_governor(
    account: &AccountSettings,
    governor: SyncGovernor,
) -> (Arc<SupervisorShared>, TempDirGuard) {
    let (shared, root) = test_shared(account);
    // `SupervisorShared` is not mutable behind the `Arc`; rebuild it with the
    // requested governor, reusing the already-constructed collaborators.
    let shared = Arc::new(SupervisorShared {
        service: shared.service.clone(),
        store: shared.store.clone(),
        secret_store: shared.secret_store.clone(),
        event_sender: shared.event_sender.clone(),
        gateways: RwLock::new(HashMap::new()),
        runtime_overviews: RwLock::new(HashMap::new()),
        runtime_generations: RwLock::new(HashMap::new()),
        sync_cycle_generations: RwLock::new(HashMap::new()),
        known_accounts: RwLock::new(HashSet::new()),
        account_count: AtomicUsize::new(0),
        cache_resources: Mutex::new(CacheResourceGovernor::new(
            Instant::now(),
            CacheResourcePolicy::default(),
        )),
        sync_governor: governor,
        poll_interval: Duration::from_secs(60),
        oauth_refresh_flights: Mutex::new(HashMap::new()),
    });
    (shared, root)
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
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);

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
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let cycle = shared.next_sync_cycle_generation(&account.id).await;

    shared
        .set_sync_progress(
            &account.id,
            generation,
            cycle,
            Some(sync_progress(SyncProgressStage::Connecting)),
        )
        .await;
    shared.mark_sync_success(&account.id, generation).await;
    // A late reporter write arriving after success:
    shared
        .set_sync_progress(
            &account.id,
            generation,
            cycle,
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
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let cycle = shared.next_sync_cycle_generation(&account.id).await;
    shared
        .set_sync_progress(
            &account.id,
            generation,
            cycle,
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
                    cycle,
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
    let (shared, _root) = test_shared(&account);
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

// PP3/D90 (ruling O6): a push (re)connect must trigger an unconditional catch-up
// incremental sync — otherwise anything that changed during the outage surfaces
// only at the next 60 s poll. The `Connected` event both flips push status and
// runs a coalesced sync cycle.
// spec: docs/L2-transport#resilientpushstream
#[tokio::test]
async fn push_connected_triggers_catch_up_sync() {
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
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

    let sync_state = SyncTriggerState::new();
    let generation = shared.next_runtime_generation(&account.id).await;
    let triggered = handle_push_event(
        &sync_state,
        &shared,
        &account,
        &account.id,
        generation,
        &mut connection,
        PushStreamEvent::Connected { transport: "ws" },
    )
    .await;

    assert!(triggered, "a reconnect resets the poll interval");
    // The catch-up sync ran (a cycle was begun) and pulled messages ...
    assert_eq!(sync_state.sync_cycle_count(), 1);
    assert!(!shared
        .service
        .list_messages(&account.id, None)
        .expect("messages should list")
        .is_empty());
    // ... and push status is truthful.
    assert_eq!(
        shared.runtime_overview(&account.id).await.push,
        PushStatus::Connected
    );
}

// PP6/D91: a terminal push failure marks push `Unsupported` with a truthful
// reason and leaves the account poll-only (no infinite Reconnecting cycle).
// spec: docs/L2-transport#resilientpushstream
#[tokio::test]
async fn push_terminal_event_marks_push_unsupported_with_reason() {
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
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

    let sync_state = SyncTriggerState::new();
    let generation = shared.next_runtime_generation(&account.id).await;
    handle_push_event(
        &sync_state,
        &shared,
        &account,
        &account.id,
        generation,
        &mut connection,
        PushStreamEvent::Terminal {
            transport: "sse",
            reason: "sse permanently unavailable after 3 attempts".to_string(),
        },
    )
    .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(overview.push, PushStatus::Unsupported);
    assert_eq!(
        overview.last_sync_error_code.as_deref(),
        Some("push_terminal")
    );
    assert!(overview
        .last_sync_error
        .as_deref()
        .unwrap_or_default()
        .contains("polling every"));
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn gmail_imap_idle_hint_without_changed_ids_triggers_full_observation_sync() {
    let mut account = test_account("gmail");
    account.driver = AccountDriver::ImapSmtp;
    account.transport.provider = ProviderHint::Gmail;
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);
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
            max_attempts: WATCHDOG_MAX_RESTARTS,
        },
        jitter: Arc::new(|| 0.0),
    }
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d61
#[tokio::test]
async fn stop_all_joins_cooperative_accounts_and_escalates_a_straggler() {
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);
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
    let (shared, _root) = test_shared(&account);
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
        max_attempts: WATCHDOG_MAX_RESTARTS,
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

// ---------------------------------------------------------------------------
// M26: select-loop arm budgets + monotonic snooze clock (RFC-L2-lifecycle
// D66, row 5/N17 + row 10 rider).
// ---------------------------------------------------------------------------

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d66
#[tokio::test(start_paused = true)]
async fn hung_provider_sync_degrades_the_account_but_the_loop_stays_responsive() {
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let (command_tx, command_rx) = mpsc::channel(8);
    let sync_state = SyncTriggerState::new();
    let cancel = CancellationToken::new();

    let runtime_handle = tokio::spawn(run_account_runtime(
        shared.clone(),
        account.clone(),
        generation,
        command_rx,
        sync_state,
        cancel.clone(),
        false,
    ));

    // Wait for the (fast, un-delayed) startup sync to settle the account out
    // of its initial Offline placeholder before hanging the provider.
    let mut started = false;
    for _ in 0..200 {
        if shared.runtime_overview(&account.id).await.status != AccountStatus::Offline {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        started,
        "the startup sync should complete before the hang is introduced"
    );

    // Hang every subsequent mock sync past the sync arm's budget
    // (ARM_BUDGET_SYNC = 300s) — a stand-in for a hung provider (mock future
    // pending in spirit: it never completes inside the test's bound). Kept
    // finite (not effectively-infinite) because `SYNC_DELAY_MILLIS` is a
    // process-wide static (`posthaste-engine`'s test hook, not scoped to this
    // test): with `start_paused` this test's own wall-clock exposure is
    // sub-second, but a finite cap bounds the worst case for any other test
    // in this binary that happens to race a real (unpaused) mock sync during
    // that window.
    MockJmapGateway::set_sync_delay_for_tests(600_000);
    command_tx
        .send(RuntimeCommand::TriggerOnly {
            trigger: SyncTrigger::Manual,
        })
        .await
        .expect("command channel should accept the hung trigger");

    // The command arm's `tokio::time::timeout` must fire and degrade the
    // account. Time is paused: the runtime auto-advances the virtual clock to
    // the next pending timer once every task is stalled, so this resolves
    // without a real 300s wait. (The account is still hung — the poll tick
    // keeps retrying and re-hanging every ARM_BUDGET_SYNC, so `status` itself
    // flaps back to `Syncing` between samples; the error code left behind by
    // `mark_arm_timeout` is the stable signal that the timeout fired.)
    let mut degraded = false;
    for _ in 0..120 {
        let overview = shared.runtime_overview(&account.id).await;
        if overview.last_sync_error_code.as_deref() == Some("arm_timeout") {
            degraded = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    assert!(
        degraded,
        "a hung provider sync must degrade the account once the arm budget elapses, not hang forever"
    );

    // The loop itself must still be alive and responsive: a follow-up command
    // on the SAME account, once the provider is no longer hung, is picked up
    // and completes — proving the timed-out arm did not wedge the select!
    // loop for later ticks/commands.
    MockJmapGateway::clear_sync_delay_for_tests();
    let (reply_tx, reply_rx) = oneshot::channel();
    command_tx
        .send(RuntimeCommand::Trigger {
            trigger: SyncTrigger::Manual,
            mode: SyncMode::Incremental,
            reply: reply_tx,
        })
        .await
        .expect("command channel should accept the follow-up trigger");
    let result = tokio::time::timeout(Duration::from_secs(3600), reply_rx)
        .await
        .expect(
            "a subsequent command on the same account must process, not hang behind the \
             timed-out one",
        )
        .expect("the reply channel must not be dropped");
    assert!(
        result.is_ok(),
        "the follow-up sync must succeed once the provider is responsive again: {result:?}"
    );

    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(3600), runtime_handle).await;
    MockJmapGateway::clear_sync_delay_for_tests();
}

// ---------------------------------------------------------------------------
// M27 sub-unit (d): bounded progress-forwarding + the sync-cycle generation
// gate (RFC-L2-lifecycle N5 + the M26 flag). Extends the M26 hung-provider
// scenario above: that test proves the arm-timeout backstop degrades the
// account and keeps the loop responsive; this one proves the *other* half —
// once a cycle is abandoned, a progress write still in flight for it cannot
// undo the resulting `Degraded` status.
// ---------------------------------------------------------------------------

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67
#[tokio::test]
async fn stale_sync_cycle_write_after_arm_abandonment_cannot_revive_syncing_over_degraded() {
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;

    fn connecting_progress(sync_id: &str) -> SyncProgress {
        SyncProgress {
            sync_id: sync_id.to_string(),
            trigger: SyncTrigger::Manual,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            stage: SyncProgressStage::Connecting,
            detail: "Connecting account".to_string(),
            mailbox_name: None,
            mailbox_index: None,
            mailbox_count: None,
            message_count: None,
            total_count: None,
        }
    }

    // Cycle 1 starts (mirrors `process_sync_trigger_inner` minting its own
    // token) and writes its initial `Connecting` progress, settling status
    // to `Syncing` — same as the real synchronous call this stands in for.
    let abandoned_cycle = shared.next_sync_cycle_generation(&account.id).await;
    shared
        .set_sync_progress(
            &account.id,
            generation,
            abandoned_cycle,
            Some(connecting_progress("sync-1")),
        )
        .await;
    assert_eq!(
        shared.runtime_overview(&account.id).await.status,
        AccountStatus::Syncing
    );

    // A select!-loop arm abandons cycle 1 (mirrors `record_arm_timeout`):
    // bump the account's cycle token first, then mark it Degraded.
    shared.next_sync_cycle_generation(&account.id).await;
    shared
        .mark_arm_timeout(
            &account.id,
            generation,
            "poll_sync",
            Duration::from_secs(300),
        )
        .await;
    assert_eq!(
        shared.runtime_overview(&account.id).await.status,
        AccountStatus::Degraded
    );

    // A progress write still carrying cycle 1's now-stale token — standing in
    // for the detached forwarder task's in-flight write racing in after
    // abandonment — must be rejected outright, not flip status back to
    // `Syncing` (the M26 flag / the flap this sub-unit closes). `Connecting`
    // is deliberately used here: it is the one progress stage that already
    // bypasses the older "current status must be Syncing" guard in
    // `set_sync_progress`, so it is the write the *cycle* gate — not any
    // pre-existing status check — must be the thing that stops.
    shared
        .set_sync_progress(
            &account.id,
            generation,
            abandoned_cycle,
            Some(connecting_progress("sync-1")),
        )
        .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(
        overview.status,
        AccountStatus::Degraded,
        "a stale-cycle write from an abandoned cycle must not revive Syncing over Degraded"
    );

    // A fresh cycle's own write (the next legitimate retry) is unaffected —
    // the gate only rejects the *stale* token, not sync progress in general.
    let fresh_cycle = shared.next_sync_cycle_generation(&account.id).await;
    shared
        .set_sync_progress(
            &account.id,
            generation,
            fresh_cycle,
            Some(connecting_progress("sync-2")),
        )
        .await;
    assert_eq!(
        shared.runtime_overview(&account.id).await.status,
        AccountStatus::Syncing,
        "a fresh cycle's own progress write must still take effect"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67
#[tokio::test]
async fn sync_progress_reporter_forwards_a_burst_in_order_from_a_single_task() {
    // N5: previously every progress callback did its own `tokio::spawn`, with
    // no retained handle and no bound — concurrent tasks could race, so an
    // earlier progress value could land after a later one. Now there is one
    // forwarder task per cycle draining a bounded channel in order.
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let cycle = shared.next_sync_cycle_generation(&account.id).await;

    // Seed status to `Syncing` first, the way `process_sync_trigger_inner`'s
    // own synchronous `Connecting` write always does before the reporter
    // (and therefore the forwarder) is even created — a `Fetching`-stage
    // write only takes effect while status is already `Syncing`.
    shared
        .set_sync_progress(
            &account.id,
            generation,
            cycle,
            Some(SyncProgress {
                sync_id: "sync-1".to_string(),
                trigger: SyncTrigger::Manual,
                started_at: "2026-01-01T00:00:00Z".to_string(),
                stage: SyncProgressStage::Connecting,
                detail: "Connecting account".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: None,
                total_count: None,
            }),
        )
        .await;

    let reporter = crate::supervisor::sync_flow::sync_progress_reporter(
        &shared,
        account.id.clone(),
        generation,
        cycle,
        "sync-1".to_string(),
        SyncTrigger::Manual,
        "2026-01-01T00:00:00Z".to_string(),
    );

    // A burst of progress events, standing in for a chatty sync. Yielding
    // between reports lets the single forwarder task drain each one before
    // the next arrives, so every update — not just a lucky subset — is
    // expected to land, strictly in order.
    for index in 0..50usize {
        reporter.report(SyncProgress {
            sync_id: String::new(), // overwritten by `report`
            trigger: SyncTrigger::Manual,
            started_at: String::new(),
            stage: SyncProgressStage::Fetching,
            detail: format!("batch {index}"),
            mailbox_name: None,
            mailbox_index: Some(index),
            mailbox_count: Some(50),
            message_count: None,
            total_count: None,
        });
        // Give the single forwarder task a real scheduling chance to drain
        // this update (including its store write) before the next one is
        // sent, so the channel's small capacity is never the bottleneck.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    let mut settled = false;
    for _ in 0..200 {
        let overview = shared.runtime_overview(&account.id).await;
        if overview
            .sync_progress
            .as_ref()
            .and_then(|progress| progress.mailbox_index)
            == Some(49)
        {
            settled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        settled,
        "the forwarder should deliver every progress update in order, ending on the last one reported"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d66
#[test]
fn anchored_wall_clock_never_regresses_as_the_monotonic_elapsed_grows() {
    // The anchor's wall-clock sample is taken exactly once; every later call
    // only adds a monotonic `Instant`-derived elapsed on top. A hypothetical
    // backward jump in the *real* system clock after the anchor was taken
    // cannot be observed here — this pure function never re-samples
    // `SystemTime` — so feeding it a strictly increasing `elapsed` sequence
    // must produce a strictly increasing result. That is the guarantee row 10
    // needs: a due snooze cannot become un-due (no starving), and an
    // already-passed boundary cannot re-open (no double-firing), for as long
    // as this process runs.
    let anchor_wall = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let t0 = SupervisorShared::anchored_now_secs(anchor_wall, Duration::from_secs(0));
    let t1 = SupervisorShared::anchored_now_secs(anchor_wall, Duration::from_secs(30));
    let t2 = SupervisorShared::anchored_now_secs(anchor_wall, Duration::from_secs(31));

    assert_eq!(t0, 1_700_000_000);
    assert_eq!(t1, 1_700_000_030);
    assert_eq!(t2, 1_700_000_031);
    assert!(
        t1 > t0 && t2 > t1,
        "now must strictly advance as elapsed grows, never regress"
    );
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d66
#[tokio::test]
async fn snooze_tick_still_returns_a_due_message_on_the_monotonic_anchored_clock() {
    // Regression coverage for moving handle_snooze_tick off a raw
    // SystemTime::now() sample (row 10 rider): the monotonic-anchored clock
    // must still recognize a genuinely-due snooze as due and return it.
    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let sync_state = SyncTriggerState::new();
    let mut connection = AccountRuntimeConnectionState::default();

    // Seed the local store from the mock provider's sample mailboxes/messages.
    process_sync_trigger_with_state(
        &sync_state,
        &shared,
        &account,
        generation,
        SyncTriggerRequest::new(SyncTrigger::Startup, SyncMode::Incremental),
        &mut connection,
    )
    .await;

    let messages = shared
        .service
        .list_messages(&account.id, None)
        .expect("messages should list");
    let message = messages
        .first()
        .expect("the mock gateway seeds at least one message");

    // Due five seconds ago on the monotonic-anchored clock.
    let due_at = SupervisorShared::monotonic_now_secs() - 5;
    shared
        .store
        .insert_snooze(&account.id, &message.id, due_at)
        .expect("snooze insert should succeed");

    handle_snooze_tick(&shared, &account.id).await;

    let inbox = shared
        .service
        .list_mailboxes(&account.id)
        .expect("mailboxes should list")
        .into_iter()
        .find(|mailbox| mailbox.role.as_deref() == Some("inbox"))
        .expect("the mock gateway seeds an inbox mailbox");
    let returned = shared
        .service
        .list_messages(&account.id, Some(&inbox.id))
        .expect("messages should list");
    assert!(
        returned.iter().any(|summary| summary.id == message.id),
        "a due snooze must still be returned to the inbox on the monotonic-anchored clock"
    );
}

// ---------------------------------------------------------------------------
// Field bug (RFC-L2-provider-reliability, "cache_maintenance arm wedges"):
// a slow/hung body source made the cache batch exceed ARM_BUDGET_CACHE, the
// arm timeout DROPPED the batch future (so governor feedback/backoff never
// ran), and the 2 s cache tick re-hit the hung provider forever — the account
// was wedged until an app reload. The fix bounds the batch under its own
// deadline (BODY_CACHE_BATCH_BUDGET) so it returns with partial work and
// records backoff, and additionally records a cancelled slice if the arm
// backstop ever does fire.
// ---------------------------------------------------------------------------

// spec: docs/eph/RFC-L2-provider-reliability
#[tokio::test(start_paused = true)]
async fn hung_body_source_cannot_wedge_cache_maintenance_and_recovery_needs_no_restart() {
    use posthaste_domain_model::CacheLayer;

    let account = test_account("primary");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
    let sync_state = SyncTriggerState::new();

    // A mock provider whose messages carry no inline bodies, so the sync
    // seeds `wanted` body-cache candidates that the cache worker must fetch.
    let mock = Arc::new(MockJmapGateway::default());
    mock.strip_message_bodies_for_tests();
    let gateway: SharedGateway = mock.clone();
    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway: gateway.clone(),
        push_events: None,
        remote_observation: RemoteObservationPolicy::disabled(),
        secret_resolver: Arc::new(StaticSecretResolver::new("")),
    });
    process_sync_trigger_with_state(
        &sync_state,
        &shared,
        &account,
        generation,
        SyncTriggerRequest::new(SyncTrigger::Startup, SyncMode::Incremental),
        &mut connection,
    )
    .await;
    let candidates = shared
        .store
        .list_cache_fetch_candidates(&account.id, CacheLayer::Body, 16)
        .expect("candidates should list");
    assert!(
        !candidates.is_empty(),
        "the body-less sync must seed wanted body-cache candidates"
    );

    // Phase 1 — the body source hangs. The cache arm, wrapped exactly the way
    // the runtime loop wraps it, must complete (the batch's own deadline
    // returns first); the arm-budget backstop must NOT be what fires.
    mock.set_body_fetch_delay_for_tests(Duration::from_secs(3600));
    let arm = tokio::time::timeout(
        ARM_BUDGET_CACHE,
        handle_cache_tick(
            &shared,
            &account.id,
            Some(gateway.clone()),
            CACHE_BACKGROUND_PRESSURE,
            None,
        ),
    )
    .await;
    assert!(
        arm.is_ok(),
        "the batch must return under its own deadline, not be dropped by the arm budget"
    );
    let attempts_while_hung = mock.body_fetch_attempts_for_tests();
    assert!(attempts_while_hung >= 1, "the hung fetch was attempted");

    // The governor is in backoff, so the next 2 s tick must NOT re-hit the
    // hung provider — the perpetual-recurrence half of the wedge.
    {
        let governor = shared.cache_resources.lock().await;
        assert!(
            governor.is_in_backoff(tokio::time::Instant::now().into_std()),
            "a no-progress batch against a hung source must engage backoff"
        );
    }
    handle_cache_tick(
        &shared,
        &account.id,
        Some(gateway.clone()),
        CACHE_BACKGROUND_PRESSURE,
        None,
    )
    .await;
    assert_eq!(
        mock.body_fetch_attempts_for_tests(),
        attempts_while_hung,
        "a backed-off tick must not hammer the slow provider"
    );

    // The wedged candidate was marked Failed — it left the wanted set instead
    // of being stuck Fetching forever.
    let remaining = shared
        .store
        .list_cache_fetch_candidates(&account.id, CacheLayer::Body, 16)
        .expect("candidates should list");
    assert_eq!(
        remaining.len(),
        candidates.len() - 1,
        "the cut-short candidate must leave the wanted set (Failed), not leak as Fetching"
    );

    // Phase 2 — the provider recovers. Once the backoff expires (virtual
    // time), the next tick fetches and caches again: the account recovers
    // WITHOUT any restart or reload.
    mock.clear_body_fetch_delay_for_tests();
    tokio::time::sleep(Duration::from_secs(6)).await; // past the first 5 s backoff
    handle_cache_tick(
        &shared,
        &account.id,
        Some(gateway.clone()),
        CACHE_BACKGROUND_PRESSURE,
        None,
    )
    .await;
    assert!(
        mock.body_fetch_attempts_for_tests() > attempts_while_hung,
        "after backoff + recovery the cache worker fetches again without a restart"
    );
    let governor = shared.cache_resources.lock().await;
    assert!(
        !governor.is_in_backoff(tokio::time::Instant::now().into_std()),
        "a successful batch clears the backoff"
    );
}

// ---------------------------------------------------------------------------
// M36 / D98 (Sc1 / R4): startup splay + the global concurrent-sync cap.
// ---------------------------------------------------------------------------

// spec: docs/eph/RFC-L2-provider-reliability#d98
#[tokio::test]
async fn boot_storm_never_exceeds_the_global_concurrent_sync_cap() {
    // A boot storm: N accounts started in a tight loop each fire an immediate
    // Startup sync. With the global cap, at most CAP provider syncs run at once;
    // the rest queue on the governor's semaphore. Splay is pinned to ZERO so the
    // CAP — not the splay — is the binding constraint under test.
    const CAP: usize = 3;
    const ACCOUNTS: usize = 12;

    let seed = test_account("bootstorm-seed");
    let (shared, _root) =
        test_shared_with_governor(&seed, SyncGovernor::for_test(CAP, Duration::ZERO));
    let supervisor = AccountSupervisor::from_shared_for_test(shared.clone());

    // Gate every account's provider pull at entry: an admitted sync (one that
    // has acquired a global slot) blocks there, so exactly CAP syncs sit
    // in-flight at once and the rest wait on the semaphore — a deterministic
    // stand-in for a slow provider, with no reliance on the process-wide sync
    // delay (which another test in this binary also drives).
    let _probe = MockJmapGateway::install_sync_concurrency_probe_for_tests("bootstorm-");
    let release = Arc::new(tokio::sync::Notify::new());
    let mut gates = Vec::new();
    let mut ids = Vec::new();
    for i in 0..ACCOUNTS {
        let account = test_account(&format!("bootstorm-{i}"));
        shared
            .service
            .save_source(&account)
            .expect("account should save");
        gates.push(MockJmapGateway::gate_sync_at_entry(
            &account.id,
            Arc::new(tokio::sync::Notify::new()),
            release.clone(),
        ));
        ids.push(account.id.clone());
        // Immediate start (no splay), the boot-loop shape.
        supervisor.start_account(&account).await;
    }

    // Wait until the cap is saturated (proves the cap actually binds), bounded.
    let saturated = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if MockJmapGateway::observed_peak_concurrent_syncs_for_tests() >= CAP {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    // Give any (incorrectly) over-cap sync a chance to slip through before we
    // sample the peak, so the assertion is not merely observing an early moment.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let peak = MockJmapGateway::observed_peak_concurrent_syncs_for_tests();

    // Teardown: cancel the gate-blocked syncs and drain the runtimes.
    drop(gates);
    supervisor.stop_all(Duration::from_secs(2)).await;

    assert!(
        saturated.is_ok(),
        "the {ACCOUNTS} startup syncs should saturate the cap of {CAP} within the bound"
    );
    assert!(
        peak <= CAP,
        "at most {CAP} provider syncs may run concurrently across accounts, observed peak {peak}"
    );
    assert_eq!(
        peak, CAP,
        "the global cap must be the binding constraint under a boot storm of {ACCOUNTS} accounts"
    );
}

// spec: docs/eph/RFC-L2-provider-reliability#d98
#[test]
fn startup_splay_delay_stays_within_the_configured_window() {
    // (a) A zero window disables the splay entirely (the interactive path).
    let seed = test_account("splay");
    let (immediate, _root0) =
        test_shared_with_governor(&seed, SyncGovernor::for_test(GLOBAL_CONCURRENT_SYNC_LIMIT, Duration::ZERO));
    assert_eq!(immediate.startup_splay_delay(), Duration::ZERO);

    // (b) A non-zero window yields a jittered delay strictly inside it, so N
    // accounts do not all splay to the same instant.
    let window = Duration::from_secs(4);
    let (splayed, _root1) =
        test_shared_with_governor(&seed, SyncGovernor::for_test(GLOBAL_CONCURRENT_SYNC_LIMIT, window));
    for _ in 0..64 {
        assert!(
            splayed.startup_splay_delay() < window,
            "the startup splay must fall within [0, window)"
        );
    }
}

// ---------------------------------------------------------------------------
// M37 / D101-D102: OAuth refresh CAS rotation (A1) + invalid_grant → AuthError
// propagation from the refresh tick (A2).
// ---------------------------------------------------------------------------

/// A secret resolver whose refresh always fails — `auth` chooses between the
/// terminal `invalid_grant`/`unauthorized_client` class (`GatewayError::Auth`)
/// and a transient network blip. Stands in for the OAuth token endpoint so the
/// refresh tick's classification path can be driven without a live IdP.
#[derive(Debug)]
struct RefreshFailResolver {
    auth: bool,
}

#[async_trait::async_trait]
impl SecretResolver for RefreshFailResolver {
    async fn resolve_secret(&self) -> Result<String, GatewayError> {
        if self.auth {
            Err(GatewayError::Auth)
        } else {
            Err(GatewayError::Network("transient refresh blip".to_string()))
        }
    }
}

/// In-memory `SecretStore` counting `resolve` calls, so a test can exercise the
/// default compare-and-swap (`update_if_unchanged`) rotation semantics that the
/// real refresh path depends on (D101 / A1).
struct CountingSecretStore {
    values: std::sync::Mutex<std::collections::HashMap<String, String>>,
    resolves: AtomicUsize,
}

impl CountingSecretStore {
    fn seeded(key: &str, value: &str) -> Self {
        let mut values = std::collections::HashMap::new();
        values.insert(key.to_string(), value.to_string());
        Self {
            values: std::sync::Mutex::new(values),
            resolves: AtomicUsize::new(0),
        }
    }

    fn current(&self, key: &str) -> String {
        self.values.lock().unwrap().get(key).cloned().unwrap_or_default()
    }
}

impl SecretStore for CountingSecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.resolves.fetch_add(1, Ordering::SeqCst);
        self.values
            .lock()
            .unwrap()
            .get(&secret_ref.key)
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable(secret_ref.key.clone()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .unwrap()
            .insert(secret_ref.key.clone(), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.values.lock().unwrap().remove(&secret_ref.key);
        Ok(())
    }
}

fn oauth_account(id: &str) -> AccountSettings {
    let mut account = test_account(id);
    account.transport.auth = ProviderAuthKind::OAuth2;
    account
}

/// A2 / D102: a proactive refresh that fails `invalid_grant` (classified
/// `GatewayError::Auth`, a Permanent `Terminality`) flips the account to
/// `AuthError` *from the tick* — not on some later connection rebuild.
#[tokio::test]
async fn oauth_refresh_tick_flips_autherror_on_invalid_grant() {
    let account = oauth_account("oauth-revoked");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
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

    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway: Arc::new(MockJmapGateway::default()),
        push_events: None,
        remote_observation: RemoteObservationPolicy::disabled(),
        secret_resolver: Arc::new(RefreshFailResolver { auth: true }),
    });
    let mut state = OAuthRefreshState::new(&account);

    handle_oauth_refresh_tick(
        &shared,
        &account,
        generation,
        &account.id,
        &mut connection,
        &mut state,
    )
    .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(
        overview.status,
        AccountStatus::AuthError,
        "a revoked grant must surface as AuthError immediately from the refresh tick",
    );
    assert_eq!(
        overview.last_sync_error_code.as_deref(),
        Some("auth_error"),
    );
}

/// A2 negative: a *transient* refresh failure (a network blip) must NOT flip
/// `AuthError` — it is logged and retried on the next tick, leaving status
/// untouched so a momentary IdP hiccup never masquerades as "needs re-auth".
#[tokio::test]
async fn oauth_refresh_tick_keeps_status_on_transient_error() {
    let account = oauth_account("oauth-blip");
    let (shared, _root) = test_shared(&account);
    let generation = shared.next_runtime_generation(&account.id).await;
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

    let mut connection = AccountRuntimeConnectionState::default();
    connection.set_connected(AccountConnection {
        gateway: Arc::new(MockJmapGateway::default()),
        push_events: None,
        remote_observation: RemoteObservationPolicy::disabled(),
        secret_resolver: Arc::new(RefreshFailResolver { auth: false }),
    });
    let mut state = OAuthRefreshState::new(&account);

    handle_oauth_refresh_tick(
        &shared,
        &account,
        generation,
        &account.id,
        &mut connection,
        &mut state,
    )
    .await;

    let overview = shared.runtime_overview(&account.id).await;
    assert_eq!(
        overview.status,
        AccountStatus::Ready,
        "a transient refresh error must not flip the account to AuthError",
    );
}

/// A1 / D101: two refreshes both read the same stored token set (`gen0`), both
/// rotate and try to persist. The compare-and-swap lets exactly ONE land; the
/// loser's stale write is rejected and it re-reads the winner's token rather
/// than clobbering the freshly-rotated refresh token (the permanent-lockout
/// race). This models `refresh_oauth_access_token`'s CAS-write / CAS-miss arms
/// at the store seam.
#[tokio::test]
async fn oauth_refresh_cas_rejects_losing_writer_and_keeps_winner_token() {
    fn token_set(access: &str, refresh: &str) -> String {
        OAuthTokenSet {
            r#type: crate::oauth::oauth_secret_type(),
            provider: ProviderHint::Gmail,
            client_id: "client".to_string(),
            client_secret: Some("secret".to_string()),
            access_token: access.to_string(),
            refresh_token: Some(refresh.to_string()),
            expires_at: Some("2026-04-27T10:00:00Z".to_string()),
            scopes: vec!["https://mail.google.com/".to_string()],
        }
        .encode()
        .expect("encode token set")
    }

    let secret_ref = SecretRef {
        kind: posthaste_domain_model::SecretKind::Os,
        key: "acct-oauth".to_string(),
    };
    let gen0 = token_set("access-0", "refresh-0");
    let store = CountingSecretStore::seeded(&secret_ref.key, &gen0);

    // Both racers read gen0.
    let seen_by_a = store.resolve(&secret_ref).expect("resolve a");
    let seen_by_b = store.resolve(&secret_ref).expect("resolve b");
    assert_eq!(seen_by_a, gen0);
    assert_eq!(seen_by_b, gen0);

    // Winner (A) rotates gen0 -> gen1 and its CAS lands.
    let gen1 = token_set("access-1", "refresh-1");
    let outcome_a = store
        .update_if_unchanged(&secret_ref, &seen_by_a, &gen1)
        .expect("cas a");
    assert_eq!(outcome_a, SecretCasOutcome::Swapped);

    // Loser (B) rotated gen0 -> gen2, but its CAS expects the now-stale gen0.
    let gen2 = token_set("access-2", "refresh-2");
    let outcome_b = store
        .update_if_unchanged(&secret_ref, &seen_by_b, &gen2)
        .expect("cas b");
    // The stale write is REJECTED, and the CAS returns the winner's set so the
    // loser adopts it instead of re-refreshing a consumed grant.
    let SecretCasOutcome::Mismatch { current } = outcome_b else {
        panic!("losing writer's stale CAS must miss, got {outcome_b:?}");
    };
    assert_eq!(current, gen1, "CAS-miss must carry the winner's token set");

    // The store still holds the winner's token — no last-writer-wins loss.
    assert_eq!(store.current(&secret_ref.key), gen1);
    let winner = OAuthTokenSet::decode(&current).expect("decode winner");
    assert_eq!(winner.access_token, "access-1");
    assert_eq!(winner.refresh_token.as_deref(), Some("refresh-1"));
}

/// M34 single-flight (verify unregressed): the per-secret-ref refresh flight on
/// `SupervisorShared` serializes concurrent refreshes of the same ref. Two tasks
/// acquiring the same ref's flight never overlap; distinct refs get distinct
/// locks and run freely.
#[tokio::test]
async fn oauth_refresh_single_flight_serializes_same_ref() {
    let account = oauth_account("oauth-flight");
    let (shared, _root) = test_shared(&account);
    let key = "acct-flight".to_string();

    // Same key hands back the same flight lock (one in-flight refresh per ref).
    let flight_a = {
        let mut flights = shared.oauth_refresh_flights.lock().await;
        Arc::clone(flights.entry(key.clone()).or_default())
    };
    let flight_b = {
        let mut flights = shared.oauth_refresh_flights.lock().await;
        Arc::clone(flights.entry(key.clone()).or_default())
    };
    assert!(
        Arc::ptr_eq(&flight_a, &flight_b),
        "the same secret ref must share one refresh flight",
    );

    // Mutual exclusion: while one task holds the flight, a second cannot enter.
    let concurrency = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let shared = shared.clone();
        let key = key.clone();
        let concurrency = concurrency.clone();
        let peak = peak.clone();
        handles.push(tokio::spawn(async move {
            let flight = {
                let mut flights = shared.oauth_refresh_flights.lock().await;
                Arc::clone(flights.entry(key).or_default())
            };
            let _guard = flight.lock().await;
            let now = concurrency.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            concurrency.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for handle in handles {
        handle.await.expect("flight task should not panic");
    }
    assert_eq!(
        peak.load(Ordering::SeqCst),
        1,
        "the single-flight must admit at most one refresh of a ref at a time",
    );
}
