//! Assembly integration tests: the service core stands up against a temp
//! directory, runs a mock account's runtime to a healthy status, serves and
//! publishes its connection info, and tears down cleanly.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use posthaste_client_backend::{serve, AppPaths, AppState, BuildOptions, ConnectionInfo};
use posthaste_domain_model::{
    now_iso8601, AccountDriver, AccountId, AccountSettings, AccountStatus,
    AccountTransportSettings, AutomationAction, AutomationBackfillJobStatus, AutomationRule,
    AutomationTrigger, MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator,
    MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue, MessageSortField,
    SecretRef, SecretStoreError, SortDirection,
};
use posthaste_domain_service::SecretStore;

/// In-memory secret store so tests never touch the OS keychain.
#[derive(Default)]
struct MemorySecretStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .get(&secret_ref.key)
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable(secret_ref.key.clone()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .insert(secret_ref.key.clone(), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.secrets.lock().unwrap().remove(&secret_ref.key);
        Ok(())
    }
}

fn mock_account(id: &str) -> AccountSettings {
    let now = now_iso8601().expect("clock");
    AccountSettings {
        id: id.into(),
        name: format!("Mock {id}"),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: now.clone(),
        updated_at: now,
    }
}

async fn assemble_at(root: &std::path::Path) -> AppState {
    let paths = AppPaths::with_roots(root.join("config"), root.join("state"));
    let options = BuildOptions {
        poll_interval: Duration::from_secs(1),
        secret_store: Some(Arc::new(MemorySecretStore::default())),
        ..BuildOptions::at(paths)
    };
    AppState::assemble(options).await.expect("assembles")
}

#[tokio::test]
async fn assembles_starts_and_shuts_down_with_no_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let state = assemble_at(dir.path()).await;

    assert_eq!(state.supervisor.account_count(), 0);
    assert!(state.repair.is_none(), "a fresh database needs no repair");
    assert!(dir.path().join("state/mail.sqlite").exists());
    assert!(
        dir.path().join("config/app.toml").exists(),
        "an empty config repo is initialized with defaults"
    );

    // The generation starts at zero for this run and the run id is stable.
    assert_eq!(state.events.generation(), 0);
    let run_id = state.events.run_id().to_string();
    assert_eq!(state.events.run_id(), run_id);

    state.shutdown().await;
    // Shutdown is idempotent.
    state.shutdown().await;
}

#[tokio::test]
async fn mock_account_runtime_syncs_to_ready_and_publishes_events() {
    let dir = tempfile::tempdir().unwrap();
    let state = assemble_at(dir.path()).await;
    let mut event_rx = state.events.subscribe();

    let account = mock_account("primary");
    state.config.save_source(&account).expect("save account");
    state.service.sync_source_projections().expect("projects");
    state.supervisor.start_account(&account).await;
    assert_eq!(state.supervisor.account_count(), 1);

    // The startup sync against the mock gateway drives the account to Ready.
    let account_id = AccountId::from("primary");
    let mut status = AccountStatus::Offline;
    for _ in 0..100 {
        status = state.supervisor.runtime_overview(&account_id).await.status;
        if status == AccountStatus::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, AccountStatus::Ready, "startup sync should settle");

    // Committed writes advanced the generation and reached the bus.
    assert!(
        state.events.generation() > 0,
        "sync writes must bump the store generation"
    );
    let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("an event should be broadcast")
        .expect("bus stays open");
    assert_eq!(event.account_id.as_str(), "primary");

    // An explicit manual sync round-trips through the runtime.
    state
        .supervisor
        .sync_account(&account_id)
        .await
        .expect("manual sync succeeds");
    assert!(state.supervisor.sync_cycle_count(&account_id).await >= 2);

    state.shutdown().await;
}

fn keyword_query(tag: &str) -> MailQueryRule {
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes: vec![MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::Keyword,
                operator: MailQueryOperator::Equals,
                negated: false,
                value: MailQueryValue::String(tag.to_string()),
            })],
        },
    }
}

#[tokio::test]
async fn automation_backfill_job_drains_through_the_account_runtime() {
    let tag = "assembly-backfilled";
    let dir = tempfile::tempdir().unwrap();
    let state = assemble_at(dir.path()).await;

    let account = mock_account("primary");
    state.config.save_source(&account).expect("save account");
    state.service.sync_source_projections().expect("projects");

    // A backfill-enabled rule matching the mock gateway's sample senders,
    // written the same way updateSettings writes it: settings document +
    // durable job ensure.
    let mut settings = state.service.get_app_settings().expect("settings read");
    settings.automation_rules = vec![AutomationRule {
        id: "rule-assembly-backfill".to_string(),
        name: "Tag existing example.com mail".to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::Manual],
        condition: MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes: vec![MailQueryRuleNode::Condition(MailQueryCondition {
                    field: MailQueryField::FromEmail,
                    operator: MailQueryOperator::Contains,
                    negated: false,
                    value: MailQueryValue::String("example.com".to_string()),
                })],
            },
        },
        actions: vec![AutomationAction::ApplyTag {
            tag: tag.to_string(),
        }],
        backfill: true,
    }];
    state
        .service
        .put_app_settings(&settings)
        .expect("settings save");
    let jobs = state
        .service
        .ensure_automation_backfills_for_current_rules()
        .expect("jobs ensured");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, AutomationBackfillJobStatus::Pending);

    state.supervisor.start_account(&account).await;

    // The runtime's backfill arm drains the durable job after the startup
    // sync settles (initial delay, then bounded batches).
    let account_id = AccountId::from("primary");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let job = state
            .service
            .automation_backfill_job_for_current_rules(&account_id)
            .expect("job readable")
            .expect("job exists");
        if job.status == AutomationBackfillJobStatus::Completed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the runtime should complete the backfill job against the mock account"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // The rule's effect is queryable through the effective views, and the
    // progress events bumped the store generation for connected clients.
    let page = state
        .service
        .query_message_page_by_rule(
            &keyword_query(tag),
            100,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .expect("tag query evaluates");
    assert!(
        !page.items.is_empty(),
        "backfill applied the rule to existing mock mail"
    );
    assert!(state.events.generation() > 0);

    state.shutdown().await;
}

#[tokio::test]
async fn serve_binds_ephemeral_port_and_connection_info_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let state = assemble_at(dir.path()).await;

    let mut info = ConnectionInfo::generate(0);
    let server = serve(state.clone(), 0, info.token.clone())
        .await
        .expect("binds loopback");
    assert_ne!(server.addr.port(), 0);

    info.port = server.addr.port();
    let info_path = state.paths.connection_info_path();
    info.write(&info_path).expect("writes connection info");
    let read: ConnectionInfo =
        serde_json::from_slice(&std::fs::read(&info_path).unwrap()).expect("parses");
    assert_eq!(read.port, server.addr.port());
    assert_eq!(read.token, info.token);

    server.abort();
    ConnectionInfo::remove(&info_path).expect("removes");
    state.shutdown().await;
}
