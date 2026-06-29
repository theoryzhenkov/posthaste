use std::sync::Arc;

use posthaste_domain::{
    AccountId, AccountSettings, MailService, SecretKind, SecretRef, SecretStore, ServiceError,
};
use posthaste_observability::{events, ph_error, ph_warn};
use posthaste_runtime_contract::{RuntimeError, SecretWriteMode, SecretWriteMutation};

pub(crate) struct AccountRepository {
    service: Arc<MailService>,
    secret_store: Arc<dyn SecretStore>,
}

impl AccountRepository {
    pub(crate) fn new(service: Arc<MailService>, secret_store: Arc<dyn SecretStore>) -> Self {
        Self {
            service,
            secret_store,
        }
    }

    /// Pure: compute the secret reference the account should carry after the
    /// requested secret mutation. Callers set this value before validating and
    /// persisting the account.
    pub(crate) fn resolve_secret_ref(
        &self,
        account_id: &AccountId,
        previous_secret_ref: Option<&SecretRef>,
        secret: &SecretWriteMutation,
    ) -> Result<Option<SecretRef>, RuntimeError> {
        Ok(
            decide_secret_instruction(account_id, previous_secret_ref, secret)?
                .resolved_secret_ref(previous_secret_ref),
        )
    }

    pub(crate) fn create(
        &self,
        account: &AccountSettings,
        secret: &SecretWriteMutation,
    ) -> Result<(), RuntimeError> {
        let decision = self.plan_secret(account, None, secret)?;
        self.service.insert_source(account)?;
        if let Err(error) = self.apply_store_instruction(&decision.store_instruction) {
            if let Err(rollback) = self.service.delete_source(&account.id) {
                let rollback = RuntimeError::from(rollback);
                ph_error!(
                    events::ACCOUNT_CREATE_COMPENSATION_FAILED,
                    account_id = %account.id,
                    original_error = %error,
                    rollback_error = %rollback,
                    "failed to roll back account after secret write failure"
                );
                return Err(RuntimeError::compensation_failed(
                    "account.create",
                    error,
                    rollback,
                ));
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn update(
        &self,
        account: &AccountSettings,
        previous_secret_ref: Option<&SecretRef>,
        secret: &SecretWriteMutation,
    ) -> Result<(), RuntimeError> {
        let decision = self.plan_secret(account, previous_secret_ref, secret)?;
        if secret.mode == SecretWriteMode::Clear {
            self.service.save_source(account)?;
            if let Err(error) = self.apply_store_instruction(&decision.store_instruction) {
                self.log_secret_delete_failure(&account.id, &error);
            }
            return Ok(());
        }

        self.apply_store_instruction(&decision.store_instruction)?;
        self.service.save_source(account)?;
        Ok(())
    }

    pub(crate) fn delete(&self, account: &AccountSettings) -> Result<(), RuntimeError> {
        self.service.delete_source(&account.id)?;
        if let Err(error) = self.delete_managed_secret(account.transport.secret_ref.as_ref()) {
            self.log_secret_delete_failure(&account.id, &error);
        }
        Ok(())
    }

    fn plan_secret<'a>(
        &self,
        account: &AccountSettings,
        previous_secret_ref: Option<&SecretRef>,
        secret: &'a SecretWriteMutation,
    ) -> Result<SecretInstructionDecision<'a>, RuntimeError> {
        let decision = decide_secret_instruction(&account.id, previous_secret_ref, secret)?;
        let resolved = decision.resolved_secret_ref(previous_secret_ref);
        if account.transport.secret_ref != resolved {
            return Err(RuntimeError::invalid_secret(
                "account secret_ref does not match secret request",
            ));
        }
        Ok(decision)
    }

    fn apply_store_instruction(
        &self,
        instruction: &SecretStoreInstruction<'_>,
    ) -> Result<(), RuntimeError> {
        match instruction {
            SecretStoreInstruction::None => Ok(()),
            SecretStoreInstruction::Save {
                secret_ref,
                password,
            } => self
                .secret_store
                .save(secret_ref, password)
                .map_err(ServiceError::from)
                .map_err(RuntimeError::from),
            SecretStoreInstruction::Update {
                secret_ref,
                password,
            } => self
                .secret_store
                .update(secret_ref, password)
                .map_err(ServiceError::from)
                .map_err(RuntimeError::from),
            SecretStoreInstruction::Delete { secret_ref } => self
                .secret_store
                .delete(secret_ref)
                .map_err(ServiceError::from)
                .map_err(RuntimeError::from),
        }
    }

    fn delete_managed_secret(&self, secret_ref: Option<&SecretRef>) -> Result<(), RuntimeError> {
        if let Some(secret_ref) = secret_ref {
            if matches!(secret_ref.kind, SecretKind::Os) {
                self.secret_store
                    .delete(secret_ref)
                    .map_err(ServiceError::from)?;
            }
        }
        Ok(())
    }

    fn log_secret_delete_failure(&self, account_id: &AccountId, error: &RuntimeError) {
        ph_warn!(
            events::ACCOUNT_SECRET_DELETE_FAILED,
            account_id = %account_id,
            error = %error,
            "account secret delete failed after account record commit"
        );
    }
}

struct SecretInstructionDecision<'a> {
    account_secret_ref: AccountSecretRefUpdate,
    store_instruction: SecretStoreInstruction<'a>,
}
impl SecretInstructionDecision<'_> {
    fn resolved_secret_ref(&self, previous_secret_ref: Option<&SecretRef>) -> Option<SecretRef> {
        match &self.account_secret_ref {
            AccountSecretRefUpdate::Preserve => previous_secret_ref.cloned(),
            AccountSecretRefUpdate::Set(secret_ref) => secret_ref.clone(),
        }
    }
}

enum AccountSecretRefUpdate {
    Preserve,
    Set(Option<SecretRef>),
}

enum SecretStoreInstruction<'a> {
    None,
    Save {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Update {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Delete {
        secret_ref: SecretRef,
    },
}

fn decide_secret_instruction<'a>(
    account_id: &AccountId,
    previous_secret_ref: Option<&SecretRef>,
    secret: &'a SecretWriteMutation,
) -> Result<SecretInstructionDecision<'a>, RuntimeError> {
    validate_secret_request(secret)?;
    let decision = match secret.mode {
        SecretWriteMode::Keep => SecretInstructionDecision {
            account_secret_ref: previous_secret_ref
                .cloned()
                .map(|secret_ref| AccountSecretRefUpdate::Set(Some(secret_ref)))
                .unwrap_or(AccountSecretRefUpdate::Preserve),
            store_instruction: SecretStoreInstruction::None,
        },
        SecretWriteMode::Replace => {
            let password = required_secret_password(secret)?;
            let secret_ref = previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .unwrap_or_else(|| account_secret_ref(account_id));
            let store_instruction = match previous_secret_ref {
                Some(existing) if existing == &secret_ref => SecretStoreInstruction::Update {
                    secret_ref: secret_ref.clone(),
                    password,
                },
                _ => SecretStoreInstruction::Save {
                    secret_ref: secret_ref.clone(),
                    password,
                },
            };
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(secret_ref)),
                store_instruction,
            }
        }
        SecretWriteMode::Clear => SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .map(|secret_ref| SecretStoreInstruction::Delete { secret_ref })
                .unwrap_or(SecretStoreInstruction::None),
        },
    };
    Ok(decision)
}

fn validate_secret_request(secret: &SecretWriteMutation) -> Result<(), RuntimeError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(RuntimeError::invalid_secret(
                    "secret.password is only allowed when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Replace => {
            required_secret_password(secret)?;
        }
        SecretWriteMode::Clear => {
            if secret.password.is_some() {
                return Err(RuntimeError::invalid_secret(
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

fn required_secret_password(secret: &SecretWriteMutation) -> Result<&str, RuntimeError> {
    secret
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::invalid_secret("secret.password is required when secret.mode is replace")
        })
}

fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use posthaste_domain::{
        AccountDriver, AccountTransportSettings, AppSettings, ConfigDiff, ConfigError,
        ConfigRepository, ConfigSnapshot, SecretStoreError, SmartMailbox, SmartMailboxId,
        RFC3339_EPOCH,
    };
    use posthaste_runtime_contract::RuntimeErrorCode;
    use posthaste_store::DatabaseStore;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("posthaste-account-repository-test-{now}-{seq}"))
    }

    #[derive(Default)]
    struct TestConfig {
        sources: Mutex<Vec<AccountSettings>>,
        app_settings: Mutex<AppSettings>,
        fail_save: AtomicBool,
        fail_delete: AtomicBool,
    }

    impl TestConfig {
        fn source(&self, id: &str) -> Option<AccountSettings> {
            self.sources
                .lock()
                .expect("sources lock poisoned")
                .iter()
                .find(|source| source.id.as_str() == id)
                .cloned()
        }

        fn fail_next_save(&self) {
            self.fail_save.store(true, Ordering::SeqCst);
        }

        fn fail_next_delete(&self) {
            self.fail_delete.store(true, Ordering::SeqCst);
        }
    }

    impl ConfigRepository for TestConfig {
        fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
            Ok(ConfigSnapshot {
                app_settings: self.get_app_settings()?,
                sources: self.list_sources()?,
                smart_mailboxes: Vec::new(),
            })
        }

        fn reload(&self) -> Result<ConfigDiff, ConfigError> {
            Ok(ConfigDiff {
                added_sources: Vec::new(),
                changed_sources: Vec::new(),
                removed_sources: Vec::new(),
            })
        }

        fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
            Ok(self
                .app_settings
                .lock()
                .expect("app settings lock poisoned")
                .clone())
        }

        fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError> {
            *self
                .app_settings
                .lock()
                .expect("app settings lock poisoned") = settings.clone();
            Ok(())
        }

        fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError> {
            Ok(self.sources.lock().expect("sources lock poisoned").clone())
        }

        fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError> {
            Ok(self
                .sources
                .lock()
                .expect("sources lock poisoned")
                .iter()
                .find(|source| &source.id == id)
                .cloned())
        }

        fn insert_source(&self, source: &AccountSettings) -> Result<(), ConfigError> {
            let mut sources = self.sources.lock().expect("sources lock poisoned");
            if sources.iter().any(|existing| existing.id == source.id) {
                return Err(ConfigError::Conflict(format!(
                    "account '{}' already exists",
                    source.id
                )));
            }
            sources.push(source.clone());
            Ok(())
        }

        fn save_source(&self, source: &AccountSettings) -> Result<(), ConfigError> {
            if self.fail_save.swap(false, Ordering::SeqCst) {
                return Err(ConfigError::Io("save failed".to_string()));
            }
            let mut sources = self.sources.lock().expect("sources lock poisoned");
            if let Some(existing) = sources.iter_mut().find(|existing| existing.id == source.id) {
                *existing = source.clone();
            } else {
                sources.push(source.clone());
            }
            Ok(())
        }

        fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
            if self.fail_delete.swap(false, Ordering::SeqCst) {
                return Err(ConfigError::Io("delete failed".to_string()));
            }
            self.sources
                .lock()
                .expect("sources lock poisoned")
                .retain(|source| &source.id != id);
            Ok(())
        }

        fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
            Ok(Vec::new())
        }

        fn get_smart_mailbox(
            &self,
            _id: &SmartMailboxId,
        ) -> Result<Option<SmartMailbox>, ConfigError> {
            Ok(None)
        }

        fn save_smart_mailbox(&self, _mailbox: &SmartMailbox) -> Result<(), ConfigError> {
            Ok(())
        }

        fn delete_smart_mailbox(&self, _id: &SmartMailboxId) -> Result<(), ConfigError> {
            Ok(())
        }

        fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct TestSecretStore {
        values: Mutex<HashMap<String, String>>,
        fail_save: AtomicBool,
        fail_update: AtomicBool,
        fail_delete: AtomicBool,
    }

    impl TestSecretStore {
        fn value(&self, secret_ref: &SecretRef) -> Option<String> {
            self.values
                .lock()
                .expect("secret store lock poisoned")
                .get(&secret_key(secret_ref))
                .cloned()
        }

        fn fail_next_save(&self) {
            self.fail_save.store(true, Ordering::SeqCst);
        }

        fn fail_next_update(&self) {
            self.fail_update.store(true, Ordering::SeqCst);
        }

        fn fail_next_delete(&self) {
            self.fail_delete.store(true, Ordering::SeqCst);
        }
    }

    impl SecretStore for TestSecretStore {
        fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
            self.value(secret_ref)
                .ok_or_else(|| SecretStoreError::Unavailable("secret not found".to_string()))
        }

        fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
            if self.fail_save.swap(false, Ordering::SeqCst) {
                return Err(SecretStoreError::Unavailable("save failed".to_string()));
            }
            self.values
                .lock()
                .expect("secret store lock poisoned")
                .insert(secret_key(secret_ref), value.to_string());
            Ok(())
        }

        fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
            if self.fail_update.swap(false, Ordering::SeqCst) {
                return Err(SecretStoreError::Unavailable("update failed".to_string()));
            }
            self.save(secret_ref, value)
        }

        fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
            if self.fail_delete.swap(false, Ordering::SeqCst) {
                return Err(SecretStoreError::Unavailable("delete failed".to_string()));
            }
            self.values
                .lock()
                .expect("secret store lock poisoned")
                .remove(&secret_key(secret_ref));
            Ok(())
        }
    }

    fn secret_key(secret_ref: &SecretRef) -> String {
        format!("{:?}:{}", secret_ref.kind, secret_ref.key)
    }

    fn repository() -> (AccountRepository, Arc<TestConfig>, Arc<TestSecretStore>) {
        let root = temp_root();
        let state_root = root.join("state");
        let store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config = Arc::new(TestConfig::default());
        let service = Arc::new(MailService::new(store, config.clone()));
        let secret_store = Arc::new(TestSecretStore::default());
        (
            AccountRepository::new(service, secret_store.clone()),
            config,
            secret_store,
        )
    }

    fn test_account(id: &str) -> AccountSettings {
        AccountSettings {
            id: AccountId::from(id),
            name: "Original".to_string(),
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

    fn replace_secret(value: &str) -> SecretWriteMutation {
        SecretWriteMutation {
            mode: SecretWriteMode::Replace,
            password: Some(value.to_string()),
        }
    }

    fn clear_secret() -> SecretWriteMutation {
        SecretWriteMutation {
            mode: SecretWriteMode::Clear,
            password: None,
        }
    }

    #[test]
    fn create_rolls_back_source_when_secret_write_fails() {
        let (repo, config, secret_store) = repository();
        secret_store.fail_next_save();
        let secret = replace_secret("new-password");
        let mut account = test_account("create-fails");
        account.transport.secret_ref = repo
            .resolve_secret_ref(&account.id, None, &secret)
            .expect("secret ref should resolve");

        let error = repo
            .create(&account, &secret)
            .expect_err("secret save failure should fail create");

        assert_eq!(error.envelope().code, RuntimeErrorCode::SecretUnavailable);
        assert!(config.source("create-fails").is_none());
        assert!(secret_store
            .value(&account_secret_ref(&account.id))
            .is_none());
    }

    #[test]
    fn create_reports_structured_error_when_compensation_fails() {
        let (repo, config, secret_store) = repository();
        secret_store.fail_next_save();
        config.fail_next_delete();
        let secret = replace_secret("new-password");
        let mut account = test_account("rollback-fails");
        account.transport.secret_ref = repo
            .resolve_secret_ref(&account.id, None, &secret)
            .expect("secret ref should resolve");

        let error = repo
            .create(&account, &secret)
            .expect_err("rollback failure should surface structured error");

        assert_eq!(error.envelope().code, RuntimeErrorCode::SecretUnavailable);
        assert_eq!(
            error.envelope().details["compensation"]["operation"],
            "account.create"
        );
        assert_eq!(
            error.envelope().details["compensation"]["original"]["code"],
            "secret_unavailable"
        );
        assert_eq!(
            error.envelope().details["compensation"]["rollback"]["code"],
            "config_io"
        );
        assert!(config.source("rollback-fails").is_some());
    }

    #[test]
    fn update_replace_aborts_before_source_save_when_secret_write_fails() {
        let (repo, config, secret_store) = repository();
        let original_secret_ref = account_secret_ref(&AccountId::from("replace-fails"));
        secret_store
            .save(&original_secret_ref, "old-password")
            .expect("old secret should save");
        let mut original = test_account("replace-fails");
        original.transport.secret_ref = Some(original_secret_ref.clone());
        repo.service
            .insert_source(&original)
            .expect("original source should insert");

        secret_store.fail_next_update();
        let secret = replace_secret("new-password");
        let mut updated = original.clone();
        updated.name = "Updated".to_string();
        updated.transport.secret_ref = repo
            .resolve_secret_ref(&updated.id, original.transport.secret_ref.as_ref(), &secret)
            .expect("secret ref should resolve");

        let error = repo
            .update(&updated, original.transport.secret_ref.as_ref(), &secret)
            .expect_err("secret update failure should fail update");

        assert_eq!(error.envelope().code, RuntimeErrorCode::SecretUnavailable);
        assert_eq!(config.source("replace-fails").unwrap().name, "Original");
        assert_eq!(
            secret_store.value(&original_secret_ref).as_deref(),
            Some("old-password")
        );
    }

    #[test]
    fn update_replace_leaves_source_unchanged_when_source_save_fails_after_secret_write() {
        let (repo, config, secret_store) = repository();
        let original = test_account("save-fails");
        repo.service
            .insert_source(&original)
            .expect("original source should insert");
        config.fail_next_save();
        let secret = replace_secret("new-password");
        let mut updated = original.clone();
        updated.name = "Updated".to_string();
        updated.transport.secret_ref = repo
            .resolve_secret_ref(&updated.id, original.transport.secret_ref.as_ref(), &secret)
            .expect("secret ref should resolve");
        let new_secret_ref = updated.transport.secret_ref.clone().unwrap();

        let error = repo
            .update(&updated, original.transport.secret_ref.as_ref(), &secret)
            .expect_err("source save failure should fail update");

        assert_eq!(error.envelope().code, RuntimeErrorCode::ConfigIo);
        assert_eq!(config.source("save-fails").unwrap().name, "Original");
        assert_eq!(
            secret_store.value(&new_secret_ref).as_deref(),
            Some("new-password")
        );
    }

    #[test]
    fn update_clear_saves_record_before_deleting_managed_secret() {
        let (repo, config, secret_store) = repository();
        let secret_ref = account_secret_ref(&AccountId::from("clear-secret"));
        secret_store
            .save(&secret_ref, "old-password")
            .expect("old secret should save");
        let mut original = test_account("clear-secret");
        original.transport.secret_ref = Some(secret_ref.clone());
        repo.service
            .insert_source(&original)
            .expect("original source should insert");
        let secret = clear_secret();
        let mut updated = original.clone();
        updated.transport.secret_ref = repo
            .resolve_secret_ref(&updated.id, original.transport.secret_ref.as_ref(), &secret)
            .expect("secret ref should resolve");

        repo.update(&updated, original.transport.secret_ref.as_ref(), &secret)
            .expect("clear should save account and delete secret");

        assert!(config
            .source("clear-secret")
            .unwrap()
            .transport
            .secret_ref
            .is_none());
        assert!(secret_store.value(&secret_ref).is_none());
    }

    #[test]
    fn update_clear_keeps_saved_record_when_secret_delete_fails() {
        let (repo, config, secret_store) = repository();
        let secret_ref = account_secret_ref(&AccountId::from("clear-delete-fails"));
        secret_store
            .save(&secret_ref, "old-password")
            .expect("old secret should save");
        let mut original = test_account("clear-delete-fails");
        original.transport.secret_ref = Some(secret_ref.clone());
        repo.service
            .insert_source(&original)
            .expect("original source should insert");
        secret_store.fail_next_delete();
        let secret = clear_secret();
        let mut updated = original.clone();
        updated.transport.secret_ref = repo
            .resolve_secret_ref(&updated.id, original.transport.secret_ref.as_ref(), &secret)
            .expect("secret ref should resolve");

        repo.update(&updated, original.transport.secret_ref.as_ref(), &secret)
            .expect("secret delete failure after clear should not fail committed update");

        assert!(config
            .source("clear-delete-fails")
            .unwrap()
            .transport
            .secret_ref
            .is_none());
        assert_eq!(
            secret_store.value(&secret_ref).as_deref(),
            Some("old-password")
        );
    }

    #[test]
    fn delete_removes_source_then_managed_secret() {
        let (repo, config, secret_store) = repository();
        let secret_ref = account_secret_ref(&AccountId::from("delete-account"));
        secret_store
            .save(&secret_ref, "old-password")
            .expect("old secret should save");
        let mut account = test_account("delete-account");
        account.transport.secret_ref = Some(secret_ref.clone());
        repo.service
            .insert_source(&account)
            .expect("source should insert");

        repo.delete(&account)
            .expect("delete should remove source and managed secret");

        assert!(config.source("delete-account").is_none());
        assert!(secret_store.value(&secret_ref).is_none());
    }

    #[test]
    fn delete_keeps_source_removed_when_secret_delete_fails() {
        let (repo, config, secret_store) = repository();
        let secret_ref = account_secret_ref(&AccountId::from("delete-secret-fails"));
        secret_store
            .save(&secret_ref, "old-password")
            .expect("old secret should save");
        let mut account = test_account("delete-secret-fails");
        account.transport.secret_ref = Some(secret_ref.clone());
        repo.service
            .insert_source(&account)
            .expect("source should insert");
        secret_store.fail_next_delete();

        repo.delete(&account)
            .expect("secret delete failure should not fail committed delete");

        assert!(config.source("delete-secret-fails").is_none());
        assert_eq!(
            secret_store.value(&secret_ref).as_deref(),
            Some("old-password")
        );
    }
}
