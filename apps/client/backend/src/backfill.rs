//! Automation backfill driver: drains the durable, store-backed backfill
//! jobs that settings/rule writes create, one bounded batch per supervisor
//! tick, so backfill-enabled rules apply to existing mail. Each batch runs
//! through the domain service (store work on the blocking pool, provider
//! flush through the account's gateway) and publishes its domain events so
//! the UI refreshes as the backfill progresses. Jobs are rows in the store,
//! keyed by account and rule fingerprint, so an interrupted backfill resumes
//! where it stopped after a restart, and a failed batch leaves the job
//! pending (attempts + last error recorded) for a later tick.

use std::time::Duration;

use posthaste_domain_model::{AccountId, DomainEvent};
use posthaste_domain_service::{MailGateway, MailService};
use posthaste_observability::{events, ph_info, ph_warn};

/// Messages processed per backfill batch: small enough that one batch never
/// monopolizes the account loop between sync/push/command arms.
pub(crate) const AUTOMATION_BACKFILL_BATCH_SIZE: usize = 10;

/// First tick waits out the startup sync so backfill never competes with the
/// initial mail pull.
pub(crate) const AUTOMATION_BACKFILL_INITIAL_DELAY: Duration = Duration::from_secs(10);

/// Idle cadence: how often an account checks for pending backfill work.
pub(crate) const AUTOMATION_BACKFILL_INTERVAL: Duration = Duration::from_secs(15);

/// Follow-up delay while a job reports more work: short enough that a large
/// mailbox drains steadily, long enough that every batch yields the account
/// loop to sync, push, and commands in between.
pub(crate) const AUTOMATION_BACKFILL_DRAIN_DELAY: Duration = Duration::from_secs(1);

/// Process one bounded batch of the account's pending automation backfill
/// job and publish the resulting domain events (each publish bumps the store
/// generation, so connected clients refetch live). Returns whether the job
/// still has work queued, so the caller can shorten its next delay.
///
/// A batch failure is recorded on the job by the service (the job stays
/// pending with its attempt count and error) and reported here as "no more
/// work now": the queue never wedges, and the next regular tick retries.
pub(crate) async fn process_backfill_batch(
    service: &MailService,
    account_id: &AccountId,
    gateway: &dyn MailGateway,
    batch_size: usize,
    publish: &mut (dyn FnMut(&[DomainEvent]) + Send),
) -> bool {
    match service
        .process_automation_backfill_job_batch(account_id, gateway, batch_size)
        .await
    {
        Ok(outcome) => {
            if !outcome.events.is_empty() {
                ph_info!(
                    events::SUPERVISOR_AUTOMATION_BACKFILL_COMPLETED,
                    account_id = %account_id,
                    event_count = outcome.events.len(),
                    has_more = outcome.has_more,
                    "automation backfill batch completed"
                );
                publish(&outcome.events);
            }
            outcome.ran && outcome.has_more
        }
        Err(error) => {
            ph_warn!(
                events::SUPERVISOR_AUTOMATION_BACKFILL_FAILED,
                account_id = %account_id,
                error = %error,
                "automation backfill batch failed; the job stays pending for a later tick"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use posthaste_config::TomlConfigRepository;
    use posthaste_domain_model::{
        now_iso8601, AccountDriver, AccountId, AccountSettings, AccountTransportSettings,
        AppSettings, AutomationAction, AutomationBackfillJobStatus, AutomationRule,
        AutomationTrigger, BlobId, DomainEvent, FetchedBody, GatewayError, Identity,
        MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator,
        MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue, MailboxId,
        MailboxRecord, MessageId, MessageRecord, MessageSortField, MutationOutcome, ReplyContext,
        SendFiling, SendMessageRequest, SetKeywordsCommand, SortDirection, SyncBatch, SyncCursor,
        SyncObject, SyncTrigger, ThreadId, EVENT_TOPIC_MESSAGE_UPDATED, RFC3339_EPOCH,
    };
    use posthaste_domain_service::{
        ConfigRepository, MailGateway, MailService, PushTransport, SyncProgressReporter,
    };
    use posthaste_store::DatabaseStore;

    use super::process_backfill_batch;

    const TAG: &str = "backfilled";

    /// Minimal gateway for driver tests: hands out one seed batch on the
    /// first sync (empty afterwards), accepts message mutations blindly, and
    /// can fail the next sync to exercise the batch-failure path.
    struct TestGateway {
        seed: Mutex<Option<SyncBatch>>,
        revision: AtomicU64,
        fail_next_sync: AtomicBool,
    }

    impl TestGateway {
        fn seeded(batch: SyncBatch) -> Self {
            Self {
                seed: Mutex::new(Some(batch)),
                revision: AtomicU64::new(0),
                fail_next_sync: AtomicBool::new(false),
            }
        }

        fn empty() -> Self {
            Self::seeded(SyncBatch::default())
        }

        fn fail_next_sync(&self) {
            self.fail_next_sync.store(true, Ordering::SeqCst);
        }

        fn mutation_outcome(&self) -> MutationOutcome {
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
            MutationOutcome {
                cursor: Some(SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("state-{revision}"),
                    updated_at: RFC3339_EPOCH.to_string(),
                }),
                message: None,
            }
        }
    }

    #[async_trait]
    impl MailGateway for TestGateway {
        async fn sync(
            &self,
            _account_id: &AccountId,
            _cursors: &[SyncCursor],
            _progress: Option<SyncProgressReporter>,
        ) -> Result<SyncBatch, GatewayError> {
            if self.fail_next_sync.swap(false, Ordering::SeqCst) {
                return Err(GatewayError::Network("injected sync failure".to_string()));
            }
            Ok(self
                .seed
                .lock()
                .expect("seed lock poisoned")
                .take()
                .unwrap_or_default())
        }

        async fn fetch_message_body(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<FetchedBody, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn download_blob(
            &self,
            _account_id: &AccountId,
            _blob_id: &BlobId,
        ) -> Result<Vec<u8>, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn set_keywords(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _command: &SetKeywordsCommand,
        ) -> Result<MutationOutcome, GatewayError> {
            Ok(self.mutation_outcome())
        }

        async fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _mailbox_ids: &[MailboxId],
        ) -> Result<MutationOutcome, GatewayError> {
            Ok(self.mutation_outcome())
        }

        async fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
        ) -> Result<MutationOutcome, GatewayError> {
            Ok(self.mutation_outcome())
        }

        async fn set_mailbox_role(
            &self,
            _account_id: &AccountId,
            _mailbox_id: &MailboxId,
            _expected_state: Option<&str>,
            _role: Option<&str>,
            _clear_role_from: Option<&MailboxId>,
        ) -> Result<MutationOutcome, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn create_mailbox(
            &self,
            _account_id: &AccountId,
            _name: &str,
        ) -> Result<MailboxId, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn destroy_mailbox(
            &self,
            _account_id: &AccountId,
            _mailbox_id: &MailboxId,
            _remove_emails: bool,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_reply_context(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<ReplyContext, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn send_message(
            &self,
            _account_id: &AccountId,
            _request: &SendMessageRequest,
            _consume_draft: Option<&MessageId>,
            _idempotency_key: &str,
        ) -> Result<SendFiling, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
            Vec::new()
        }
    }

    fn account(account_id: &AccountId) -> AccountSettings {
        let now = now_iso8601().expect("clock");
        AccountSettings {
            id: account_id.clone(),
            name: "Primary".to_string(),
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

    fn seed_batch(message_count: usize) -> SyncBatch {
        SyncBatch {
            mailboxes: vec![MailboxRecord {
                id: MailboxId::from("mb-inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: message_count as i64,
            }],
            messages: (0..message_count)
                .map(|index| MessageRecord {
                    id: MessageId::from(format!("bulk-{index}").as_str()),
                    source_thread_id: ThreadId::from(format!("th-bulk-{index}").as_str()),
                    subject: Some(format!("Bulk offer {index}")),
                    from_name: Some("Bulk Sender".to_string()),
                    from_email: Some("offers@bulk.example.com".to_string()),
                    received_at: format!("2026-01-01T00:00:{:02}Z", index % 60),
                    mailbox_ids: vec![MailboxId::from("mb-inbox")],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn backfill_rule() -> AutomationRule {
        AutomationRule {
            id: "rule-backfill".to_string(),
            name: "Tag existing bulk mail".to_string(),
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
                        value: MailQueryValue::String("bulk.example.com".to_string()),
                    })],
                },
            },
            actions: vec![AutomationAction::ApplyTag {
                tag: TAG.to_string(),
            }],
            backfill: true,
        }
    }

    fn tagged_rule() -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes: vec![MailQueryRuleNode::Condition(MailQueryCondition {
                    field: MailQueryField::Keyword,
                    operator: MailQueryOperator::Equals,
                    negated: false,
                    value: MailQueryValue::String(TAG.to_string()),
                })],
            },
        }
    }

    fn open_service(root: &Path) -> (Arc<MailService>, Arc<DatabaseStore>) {
        let config: Arc<dyn ConfigRepository> =
            Arc::new(TomlConfigRepository::open(root.join("config")).expect("config repo opens"));
        let store = Arc::new(
            DatabaseStore::open(root.join("state/mail.sqlite"), root.join("state"))
                .expect("store opens"),
        );
        let service = Arc::new(MailService::new(store.clone(), config));
        (service, store)
    }

    /// Fresh store + config with `message_count` rule-matching messages in
    /// base, the backfill rule in settings, and the durable job created the
    /// same way `updateSettings` creates it.
    async fn seeded(
        root: &Path,
        message_count: usize,
    ) -> (Arc<MailService>, Arc<DatabaseStore>, AccountId, TestGateway) {
        let account_id = AccountId::from("primary");
        let (service, store) = open_service(root);
        service.save_source(&account(&account_id)).expect("saves");
        service.sync_source_projections().expect("projects");

        let gateway = TestGateway::seeded(seed_batch(message_count));
        service
            .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
            .await
            .expect("seed sync applies");

        let settings = AppSettings {
            automation_rules: vec![backfill_rule()],
            ..Default::default()
        };
        service.put_app_settings(&settings).expect("settings save");
        let jobs = service
            .ensure_automation_backfills_for_current_rules()
            .expect("jobs ensured");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, AutomationBackfillJobStatus::Pending);

        (service, store, account_id, gateway)
    }

    async fn tick(
        service: &MailService,
        account_id: &AccountId,
        gateway: &TestGateway,
        batch_size: usize,
        sink: &mut Vec<DomainEvent>,
    ) -> bool {
        let mut published = Vec::new();
        let mut publish = |batch: &[DomainEvent]| published.extend_from_slice(batch);
        let has_more =
            process_backfill_batch(service, account_id, gateway, batch_size, &mut publish).await;
        sink.append(&mut published);
        has_more
    }

    fn tagged_count(service: &MailService) -> usize {
        service
            .query_message_page_by_rule(
                &tagged_rule(),
                100,
                None,
                MessageSortField::Date,
                SortDirection::Desc,
            )
            .expect("tag query evaluates")
            .items
            .len()
    }

    #[tokio::test]
    async fn backfill_job_processes_to_completion_in_bounded_batches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, _store, account_id, gateway) = seeded(dir.path(), 5).await;

        let mut events = Vec::new();
        let mut ticks = 0;
        loop {
            let has_more = tick(&service, &account_id, &gateway, 2, &mut events).await;
            ticks += 1;
            assert!(ticks <= 10, "a 5-message job must converge");
            if !has_more {
                break;
            }
        }
        assert_eq!(
            ticks, 3,
            "5 matches at batch size 2 drain in exactly 3 bounded batches"
        );
        assert_eq!(tagged_count(&service), 5, "every matching message tagged");
        assert!(
            events
                .iter()
                .filter(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
                .count()
                >= 5,
            "each applied action surfaced a message.updated progress event"
        );

        let job = service
            .automation_backfill_job_for_current_rules(&account_id)
            .expect("job readable")
            .expect("job exists");
        assert_eq!(job.status, AutomationBackfillJobStatus::Completed);

        // A completed job suppresses further work and further events.
        let mut post = Vec::new();
        assert!(!tick(&service, &account_id, &gateway, 2, &mut post).await);
        assert!(post.is_empty(), "a completed job publishes nothing");
    }

    #[tokio::test]
    async fn backfill_job_resumes_across_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let (service, store, account_id, gateway) = seeded(dir.path(), 5).await;
            let mut events = Vec::new();
            assert!(
                tick(&service, &account_id, &gateway, 2, &mut events).await,
                "first batch leaves work queued"
            );
            assert_eq!(tagged_count(&service), 2);
            drop(service);
            store.close();
        }

        // "Restart": a fresh service over the same store and config resumes
        // the same durable job where it stopped.
        let account_id = AccountId::from("primary");
        let (service, _store) = open_service(dir.path());
        let gateway = TestGateway::empty();
        let job = service
            .automation_backfill_job_for_current_rules(&account_id)
            .expect("job readable")
            .expect("job survives restart");
        assert_eq!(job.status, AutomationBackfillJobStatus::Pending);

        let mut events = Vec::new();
        let mut ticks = 0;
        loop {
            let has_more = tick(&service, &account_id, &gateway, 2, &mut events).await;
            ticks += 1;
            assert!(ticks <= 10, "the resumed job must converge");
            if !has_more {
                break;
            }
        }
        assert_eq!(
            tagged_count(&service),
            5,
            "the resumed job finishes the remaining messages without redoing done ones"
        );
        let job = service
            .automation_backfill_job_for_current_rules(&account_id)
            .expect("job readable")
            .expect("job exists");
        assert_eq!(job.status, AutomationBackfillJobStatus::Completed);
    }

    #[tokio::test]
    async fn failing_batch_records_the_error_and_the_job_recovers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, _store, account_id, gateway) = seeded(dir.path(), 3).await;

        gateway.fail_next_sync();
        let mut events = Vec::new();
        assert!(
            !tick(&service, &account_id, &gateway, 2, &mut events).await,
            "a failed batch reports no more work now (retry waits for the next tick)"
        );
        let job = service
            .automation_backfill_job_for_current_rules(&account_id)
            .expect("job readable")
            .expect("job exists");
        assert_eq!(
            job.status,
            AutomationBackfillJobStatus::Pending,
            "a failed batch leaves the job pending, not wedged"
        );
        assert_eq!(job.attempts, 1);
        assert!(job.last_error.is_some());

        // The next tick picks the job back up and completes it.
        let mut ticks = 0;
        loop {
            let has_more = tick(&service, &account_id, &gateway, 2, &mut events).await;
            ticks += 1;
            assert!(ticks <= 10, "the retried job must converge");
            if !has_more && {
                let job = service
                    .automation_backfill_job_for_current_rules(&account_id)
                    .expect("job readable")
                    .expect("job exists");
                job.status == AutomationBackfillJobStatus::Completed
            } {
                break;
            }
        }
        assert_eq!(tagged_count(&service), 3, "all matches tagged exactly once");
    }
}
