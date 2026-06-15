use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use posthaste_authority_runtime::oauth::OAuthTokenSet;
use posthaste_authority_runtime::{
    build_authority_runtime, AuthorityRuntimeBuildConfig, AuthorityRuntimeBuildError,
};
use posthaste_domain::{
    AccountDriver, AccountId, EventFilter, ImapTransportSettings, MailboxId, ProviderAuthKind,
    ProviderHint, SecretRef, SecretStore, SecretStoreError, SmtpTransportSettings,
    TransportSecurity, EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_MESSAGE_ARRIVED,
};
use posthaste_runtime_contract::{
    AccountTransportMutation, CreateAccountMutation, RuntimeCaller, RuntimeCore, RuntimeLifecycle,
    SecretWriteMode, SecretWriteMutation,
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-authority-runtime-test-{now}-{seq}"))
}

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl TestSecretStore {
    fn value(&self, secret_ref: &SecretRef) -> Option<String> {
        self.values
            .lock()
            .expect("secret store mutex")
            .get(&secret_key(secret_ref))
            .cloned()
    }
}

impl SecretStore for TestSecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .get(&secret_key(secret_ref))
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable("secret not found".to_string()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .insert(secret_key(secret_ref), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .remove(&secret_key(secret_ref));
        Ok(())
    }
}

fn secret_key(secret_ref: &SecretRef) -> String {
    format!("{:?}:{}", secret_ref.kind, secret_ref.key)
}

// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
// spec: docs/runtime/L2#runtime-builder-transport-free
// spec: docs/backend/L2#runtime-build-before-adapters
#[tokio::test]
async fn build_from_empty_roots_reports_ready_status_without_http_or_tauri() {
    let root = temp_root();
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build from empty roots");

    assert_eq!(build.runtime_status.lifecycle, RuntimeLifecycle::Ready);
    assert!(build.runtime_status.store.config_loaded);
    assert!(build.runtime_status.store.state_store_open);
    assert!(build.runtime_status.store.cache_root_ready);
    assert_eq!(build.runtime_status.account_count, 0);
    assert!(root.join("config/app.toml").exists());
    assert!(root.join("state/mail.sqlite").exists());
    assert!(root.join("cache").is_dir());

    let handle = build.handle.clone();
    let status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("runtime status should be readable through RuntimeCore");
    assert_eq!(status, build.runtime_status);

    build
        .shutdown
        .shutdown()
        .await
        .expect("shutdown should succeed for first-slice runtime");
    let stopped_status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("runtime status should remain readable after shutdown");
    assert_eq!(stopped_status.lifecycle, RuntimeLifecycle::Stopped);
}

// spec: docs/runtime/L4#authority-build-order
#[tokio::test]
async fn authority_builder_handle_supports_account_mutations() {
    let root = temp_root();
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");

    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("acct-builder".to_string()),
                name: "Builder Account".to_string(),
                driver: Some(posthaste_domain::AccountDriver::Mock),
                enabled: Some(false),
                full_name: None,
                email_patterns: Vec::new(),
                appearance: None,
                transport: AccountTransportMutation::default(),
                secret: SecretWriteMutation::default(),
            },
        )
        .await
        .expect("builder handle should support account mutations");

    assert_eq!(created.id.as_str(), "acct-builder");
    assert_eq!(created.name, "Builder Account");
}

// spec: docs/backend/L3#account-assets-runtime-backed
// spec: docs/runtime/L4#account-resource-linkage-runtime-owned
#[tokio::test]
async fn delete_account_removes_secret_config_and_publishes_event_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(secret_store.clone());

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("delete-me".to_string()),
                name: "Delete Me".to_string(),
                full_name: None,
                email_patterns: vec!["delete-me@example.com".to_string()],
                driver: Some(AccountDriver::ImapSmtp),
                enabled: Some(false),
                appearance: None,
                transport: AccountTransportMutation {
                    provider: Some(ProviderHint::Generic),
                    auth: Some(ProviderAuthKind::Password),
                    base_url: None,
                    username: Some("delete-me@example.com".to_string()),
                    imap: Some(ImapTransportSettings {
                        host: "imap.example.com".to_string(),
                        port: 993,
                        security: TransportSecurity::Tls,
                    }),
                    smtp: Some(SmtpTransportSettings {
                        host: "smtp.example.com".to_string(),
                        port: 465,
                        security: TransportSecurity::Tls,
                    }),
                },
                secret: SecretWriteMutation {
                    mode: SecretWriteMode::Replace,
                    password: Some("secret".to_string()),
                },
            },
        )
        .await
        .expect("account should be created");
    let secret_ref = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("created account should exist")
        .transport
        .secret_ref
        .expect("created account should have a secret");
    assert!(secret_store.value(&secret_ref).is_some());

    build
        .handle
        .delete_account(RuntimeCaller::test(), created.id.clone())
        .await
        .expect("runtime should delete account");

    assert!(secret_store.value(&secret_ref).is_none());
    assert!(build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .is_none());
    let events = build
        .handle
        .replay_events(
            RuntimeCaller::test(),
            EventFilter {
                account_id: Some(created.id.clone()),
                topic: Some(EVENT_TOPIC_ACCOUNT_DELETED.to_string()),
                mailbox_id: None,
                after_seq: Some(0),
            },
        )
        .await
        .expect("runtime should replay delete events");
    assert_eq!(events.len(), 1);
}

// spec: docs/backend/L3#account-mutations-runtime-backed
// spec: docs/runtime/L4#account-mutation-contract-pattern
#[tokio::test]
async fn oauth_token_persistence_writes_secret_and_patches_account_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(secret_store.clone());

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("oauth-existing".to_string()),
                name: "OAuth Existing".to_string(),
                full_name: None,
                email_patterns: vec!["user@example.com".to_string()],
                driver: Some(AccountDriver::ImapSmtp),
                enabled: Some(false),
                appearance: None,
                transport: AccountTransportMutation {
                    provider: Some(ProviderHint::Gmail),
                    auth: Some(ProviderAuthKind::Password),
                    base_url: None,
                    username: Some("user@example.com".to_string()),
                    imap: Some(ImapTransportSettings {
                        host: "imap.example.com".to_string(),
                        port: 993,
                        security: TransportSecurity::Tls,
                    }),
                    smtp: Some(SmtpTransportSettings {
                        host: "smtp.example.com".to_string(),
                        port: 465,
                        security: TransportSecurity::Tls,
                    }),
                },
                secret: SecretWriteMutation {
                    mode: SecretWriteMode::Replace,
                    password: Some("old-password".to_string()),
                },
            },
        )
        .await
        .expect("existing account should be created");

    let secret_ref = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("created account should exist")
        .transport
        .secret_ref
        .expect("created account should have a secret");
    build
        .handle
        .persist_oauth_token_set(
            created.id.clone(),
            OAuthTokenSet {
                r#type: "oauth2".to_string(),
                provider: ProviderHint::Gmail,
                client_id: "client-id".to_string(),
                client_secret: Some("client-secret".to_string()),
                access_token: "access-token".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at: Some("2026-04-27T10:00:00Z".to_string()),
                scopes: vec!["email".to_string()],
            },
        )
        .await
        .expect("OAuth token set should be persisted by runtime");

    let stored = secret_store
        .value(&secret_ref)
        .expect("OAuth token set should be written to existing managed secret");
    let decoded =
        OAuthTokenSet::decode(&stored).expect("stored secret should be an OAuth token set");
    assert_eq!(decoded.access_token, "access-token");

    let account = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("account should exist after OAuth patch");
    assert_eq!(account.transport.auth, ProviderAuthKind::OAuth2);
}

// spec: docs/runtime/L3#event-subscription-runtime-backed
#[tokio::test]
async fn event_subscription_replays_backlog_then_filters_live_events() {
    let root = temp_root();
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let filter = EventFilter {
        account_id: Some(AccountId::from("primary")),
        topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
        mailbox_id: Some(MailboxId::from("inbox")),
        after_seq: Some(0),
    };
    let replayed = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "replayed"}),
        )
        .expect("backlog event should append");
    build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("secondary"),
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "ignored"}),
        )
        .expect("non-matching backlog event should append");

    let mut subscription = build
        .handle
        .subscribe_events(RuntimeCaller::test(), filter)
        .await
        .expect("runtime should subscribe to filtered events");

    assert_eq!(subscription.replay.len(), 1);
    assert_eq!(subscription.replay[0].seq, replayed.seq);

    let ignored_live = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(&MailboxId::from("archive")),
            None,
            serde_json::json!({"kind": "ignored-live"}),
        )
        .expect("non-matching live event should append");
    build
        .api_bridge
        .event_sender
        .send(ignored_live)
        .expect("ignored live event should broadcast");
    let matching_live = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "live"}),
        )
        .expect("matching live event should append");
    build
        .api_bridge
        .event_sender
        .send(matching_live.clone())
        .expect("matching live event should broadcast");

    let received = subscription
        .live
        .next()
        .await
        .expect("matching live event should pass runtime filter");
    assert_eq!(received.seq, matching_live.seq);
}

#[tokio::test]
async fn zero_event_channel_capacity_returns_typed_build_error() {
    let root = temp_root();
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(Arc::new(TestSecretStore::default()))
    .with_event_channel_capacity(0);

    let error = match build_authority_runtime(config).await {
        Ok(_) => panic!("zero-capacity event channel should be rejected before build side effects"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        AuthorityRuntimeBuildError::InvalidConfig(_)
    ));
}
