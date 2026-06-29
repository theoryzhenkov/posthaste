use super::*;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{ConfigRepository, MailService, MailStore, SecretStore, SecretStoreError};
use posthaste_store::DatabaseStore;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

fn expect_decision<'a>(
    result: Result<SecretInstructionDecision<'a>, ApiError>,
    context: &str,
) -> SecretInstructionDecision<'a> {
    result.unwrap_or_else(|error| {
        panic!(
            "{context}, got {:?}: {}",
            error.body.code, error.body.message
        )
    })
}

fn secret_request(mode: SecretWriteMode, password: Option<&str>) -> SecretWriteRequest {
    SecretWriteRequest {
        mode,
        password: password.map(str::to_string),
    }
}

fn secret_ref(kind: SecretKind, key: &str) -> SecretRef {
    SecretRef {
        kind,
        key: key.to_string(),
    }
}

fn test_account(secret_ref: Option<SecretRef>) -> AccountSettings {
    AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        signature: None,
        email_patterns: vec!["primary@example.com".to_string()],
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            username: Some("primary@example.com".to_string()),
            secret_ref,
            imap: Some(ImapTransportSettings {
                host: "imap.example.com".to_string(),
                port: 993,
                security: posthaste_domain::TransportSecurity::Tls,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "smtp.example.com".to_string(),
                port: 587,
                security: posthaste_domain::TransportSecurity::StartTls,
            }),
            ..Default::default()
        },
        created_at: "2026-03-31T10:00:00Z".to_string(),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    }
}

struct TestAppState {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    secret_store: Arc<RecordingSecretStore>,
    _root: TestRoot,
}

fn test_app_state() -> TestAppState {
    let root = TestRoot(
        std::env::temp_dir().join(format!("posthaste-account-support-{}", Uuid::new_v4())),
    );
    let config: Arc<dyn ConfigRepository> =
        Arc::new(TomlConfigRepository::open(root.0.join("config")).expect("open config repo"));
    let database_store = Arc::new(
        DatabaseStore::open(root.0.join("mail.sqlite"), root.0.join("data"))
            .expect("open database store"),
    );
    let store: Arc<dyn MailStore> = database_store.clone();
    let service = Arc::new(MailService::new(database_store, config));
    let secret_store = Arc::new(RecordingSecretStore::default());
    let (event_sender, _) = broadcast::channel(1);
    TestAppState {
        service,
        store,
        event_sender,
        secret_store,
        _root: root,
    }
}

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SecretStoreCall {
    Save(SecretRef, String),
    Update(SecretRef, String),
    Delete(SecretRef),
}

#[derive(Default)]
struct RecordingSecretStore {
    calls: Mutex<Vec<SecretStoreCall>>,
}

impl RecordingSecretStore {
    fn calls(&self) -> Vec<SecretStoreCall> {
        self.calls.lock().expect("calls lock").clone()
    }

    fn record(&self, call: SecretStoreCall) {
        self.calls.lock().expect("calls lock").push(call);
    }
}

impl SecretStore for RecordingSecretStore {
    fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        Err(SecretStoreError::Unavailable(
            "test store does not resolve secrets".to_string(),
        ))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.record(SecretStoreCall::Save(secret_ref.clone(), value.to_string()));
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.record(SecretStoreCall::Update(
            secret_ref.clone(),
            value.to_string(),
        ));
        Ok(())
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.record(SecretStoreCall::Delete(secret_ref.clone()));
        Ok(())
    }
}

mod resource_events;
mod secret_decisions;
