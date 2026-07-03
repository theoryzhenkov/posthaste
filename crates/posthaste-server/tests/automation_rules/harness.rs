use std::sync::Arc;

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountId, AppSettings, AutomationBackfillJobStatus, AutomationRule, SyncTrigger,
};
use posthaste_domain_service::{ConfigRepository, MailService};
use posthaste_store::DatabaseStore;
use posthaste_testkit::temp_root;

use crate::builders::account;
use crate::gateway::ScriptedGateway;

pub(super) struct RuleHarness {
    // Held only to keep the temp directory alive for the harness's lifetime;
    // removed on drop.
    _root: posthaste_testkit::TempDirGuard,
    service: Arc<MailService>,
}

impl RuleHarness {
    pub(super) fn new() -> Self {
        let root = temp_root("posthaste-automation-test");
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
        Self { _root: root, service }
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

    pub(super) fn reset_backfill(&self) {
        self.service
            .reset_automation_backfills_for_current_rules()
            .expect("backfill reset should succeed");
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
