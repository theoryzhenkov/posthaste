use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_authority_runtime::{
    build_authority_runtime, AuthorityRuntimeBuildConfig, AuthorityRuntimeBuildError,
};
use posthaste_domain::{SecretRef, SecretStore, SecretStoreError};
use posthaste_runtime_contract::{
    AccountTransportMutation, CreateAccountMutation, RuntimeCaller, RuntimeCore, RuntimeLifecycle,
    SecretWriteMutation,
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
    .with_secret_store(Arc::new(TestSecretStore));

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
    .with_secret_store(Arc::new(TestSecretStore));

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

#[tokio::test]
async fn zero_event_channel_capacity_returns_typed_build_error() {
    let root = temp_root();
    let config = AuthorityRuntimeBuildConfig::new(
        root.join("config"),
        root.join("state"),
        root.join("cache"),
    )
    .with_secret_store(Arc::new(TestSecretStore))
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
