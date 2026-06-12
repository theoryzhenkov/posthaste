use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountId, AppSettings, AutomationBackfillJobStatus, AutomationRule, ConfigRepository,
    MailService, SyncTrigger,
};
use posthaste_store::DatabaseStore;

use crate::builders::account;
use crate::gateway::ScriptedGateway;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-automation-test-{now}-{seq}"))
}

pub(super) struct RuleHarness {
    service: Arc<MailService>,
}

impl RuleHarness {
    pub(super) fn new() -> Self {
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
        let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
        let service = Arc::new(MailService::new(store, config));
        Self { service }
    }

    pub(super) fn save_account(&self, id: &str, name: &str) {
        self.service
            .save_source(&account(id, name))
            .expect("account should save");
    }

    pub(super) fn save_rules(&self, rules: Vec<AutomationRule>) {
        self.service
            .put_app_settings(&AppSettings {
                default_account_id: None,
                automation_rules: rules,
                automation_drafts: Vec::new(),
                ..Default::default()
            })
            .expect("settings should save");
    }

    pub(super) async fn sync(&self, account_id: &str, gateway: &ScriptedGateway) {
        self.service
            .sync_account(
                &AccountId::from(account_id),
                SyncTrigger::Manual,
                gateway,
                None,
            )
            .await
            .expect("sync should succeed");
    }

    pub(super) async fn backfill(
        &self,
        account_id: &str,
        gateway: &ScriptedGateway,
        batch_size: usize,
    ) -> bool {
        let (_events, has_more) = self
            .service
            .backfill_automation_rules_batch(&AccountId::from(account_id), gateway, batch_size)
            .await
            .expect("backfill should succeed");
        has_more
    }

    pub(super) async fn process_backfill_job(
        &self,
        account_id: &str,
        gateway: &ScriptedGateway,
        batch_size: usize,
    ) -> (bool, bool) {
        let outcome = self
            .service
            .process_automation_backfill_job_batch(
                &AccountId::from(account_id),
                gateway,
                batch_size,
            )
            .await
            .expect("backfill job should process");
        (outcome.ran, outcome.has_more)
    }

    pub(super) fn current_backfill_status(
        &self,
        account_id: &str,
    ) -> Option<AutomationBackfillJobStatus> {
        self.service
            .automation_backfill_job_for_current_rules(&AccountId::from(account_id))
            .expect("backfill job should load")
            .map(|job| job.status)
    }

    pub(super) fn message_keywords(&self, account_id: &str, message_id: &str) -> Vec<String> {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .keywords
    }

    pub(super) fn message_mailboxes(&self, account_id: &str, message_id: &str) -> Vec<String> {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .mailbox_ids
            .into_iter()
            .map(|mailbox_id| mailbox_id.to_string())
            .collect()
    }

    pub(super) fn message_is_read(&self, account_id: &str, message_id: &str) -> bool {
        self.service
            .list_messages(&AccountId::from(account_id), None)
            .expect("messages should list")
            .into_iter()
            .find(|message| message.id.as_str() == message_id)
            .expect("message should exist")
            .is_read
    }
}
