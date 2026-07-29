use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::*;

struct TestConfig {
    smart_mailboxes: Vec<SmartMailbox>,
    sources: Vec<AccountSettings>,
    reload_diff: ConfigDiff,
    app_settings: Mutex<AppSettings>,
    deleted_sources: Mutex<Vec<AccountId>>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            smart_mailboxes: Vec::new(),
            sources: Vec::new(),
            reload_diff: ConfigDiff {
                added_sources: Vec::new(),
                changed_sources: Vec::new(),
                removed_sources: Vec::new(),
            },
            app_settings: Mutex::new(AppSettings::default()),
            deleted_sources: Mutex::new(Vec::new()),
        }
    }
}

impl ConfigRepository for TestConfig {
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
        Ok(ConfigSnapshot {
            app_settings: self.get_app_settings()?,
            sources: self.sources.clone(),
            smart_mailboxes: self.smart_mailboxes.clone(),
        })
    }

    fn reload(&self) -> Result<ConfigDiff, ConfigError> {
        Ok(self.reload_diff.clone())
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
        Ok(self.sources.clone())
    }

    fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError> {
        Ok(self.sources.iter().find(|source| &source.id == id).cloned())
    }

    fn save_source(&self, _source: &AccountSettings) -> Result<(), ConfigError> {
        Ok(())
    }

    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
        self.deleted_sources
            .lock()
            .expect("deleted sources lock poisoned")
            .push(id.clone());
        Ok(())
    }

    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        Ok(self.smart_mailboxes.clone())
    }

    fn get_smart_mailbox(&self, id: &SmartMailboxId) -> Result<Option<SmartMailbox>, ConfigError> {
        Ok(self
            .smart_mailboxes
            .iter()
            .find(|mailbox| &mailbox.id == id)
            .cloned())
    }

    fn save_smart_mailbox(&self, _mailbox: &SmartMailbox) -> Result<(), ConfigError> {
        Ok(())
    }

    fn delete_smart_mailbox(&self, _id: &SmartMailboxId) -> Result<(), ConfigError> {
        Ok(())
    }

    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        Ok(self.smart_mailboxes.clone())
    }
}

type AppliedBodyRecord = (MessageId, Option<String>, Option<String>);

struct TestStore {
    smart_mailbox_counts_error: Option<String>,
    list_mailboxes_error: Option<String>,
    projection_calls: Mutex<Vec<String>>,
    projection_deletes: Mutex<Vec<String>>,
    source_data_deletes: Mutex<Vec<String>>,
    automation_backfill_jobs: Mutex<Vec<AutomationBackfillJob>>,
    cache_candidates: Mutex<Vec<CacheCandidate>>,
    cache_signal_updates: Mutex<Vec<CacheSignalUpdate>>,
    cache_rescore_candidates: Mutex<Vec<CacheRescoreCandidate>>,
    stale_cache_rescore_requests: Mutex<Vec<(AccountId, String, usize)>>,
    stale_cache_rescore_result: usize,
    cache_priority_updates: Mutex<Vec<CachePriorityUpdate>>,
    cache_fetch_candidates: Mutex<Vec<CacheFetchCandidate>>,
    cache_state_changes: Mutex<Vec<(MessageId, CacheObjectState, Option<String>)>>,
    cache_used_bytes: Mutex<u64>,
    applied_bodies: Mutex<Vec<AppliedBodyRecord>>,
    apply_body_error: Option<String>,
    keyword_adds: Mutex<Vec<(MessageId, Vec<String>)>>,
    rule_page: Mutex<Vec<MessageSummary>>,
    mutation_state: Mutex<MutationStoreState>,
}

impl Default for TestStore {
    fn default() -> Self {
        Self {
            smart_mailbox_counts_error: None,
            list_mailboxes_error: None,
            projection_calls: Mutex::new(Vec::new()),
            projection_deletes: Mutex::new(Vec::new()),
            source_data_deletes: Mutex::new(Vec::new()),
            automation_backfill_jobs: Mutex::new(Vec::new()),
            cache_candidates: Mutex::new(Vec::new()),
            cache_signal_updates: Mutex::new(Vec::new()),
            cache_rescore_candidates: Mutex::new(Vec::new()),
            stale_cache_rescore_requests: Mutex::new(Vec::new()),
            stale_cache_rescore_result: 0,
            cache_priority_updates: Mutex::new(Vec::new()),
            cache_fetch_candidates: Mutex::new(Vec::new()),
            cache_state_changes: Mutex::new(Vec::new()),
            cache_used_bytes: Mutex::new(0),
            applied_bodies: Mutex::new(Vec::new()),
            apply_body_error: None,
            keyword_adds: Mutex::new(Vec::new()),
            rule_page: Mutex::new(Vec::new()),
            mutation_state: Mutex::new(MutationStoreState::default()),
        }
    }
}

#[derive(Default)]
struct MutationStoreState {
    cursor: Option<SyncCursor>,
    mailbox_ids: Vec<MailboxId>,
}

impl TestStore {
    fn with_message_state(cursor_state: &str, mailbox_ids: &[&str]) -> Self {
        Self {
            mutation_state: Mutex::new(MutationStoreState {
                cursor: Some(SyncCursor {
                    object_type: SyncObject::Message,
                    state: cursor_state.to_string(),
                    updated_at: crate::RFC3339_EPOCH.to_string(),
                }),
                mailbox_ids: mailbox_ids.iter().map(|id| MailboxId::from(*id)).collect(),
            }),
            ..Default::default()
        }
    }
}

impl MailboxReadStore for TestStore {
    fn list_mailboxes(&self, _account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        self.list_mailboxes_error
            .as_ref()
            .map_or(Ok(Vec::new()), |error| {
                Err(StoreError::Failure(error.clone()))
            })
    }
}

impl MailboxRoleOverrideStore for TestStore {
    fn set_mailbox_role_override(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl MessageListStore for TestStore {
    fn list_messages(
        &self,
        _account_id: &AccountId,
        _mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        Ok(Vec::new())
    }

    fn list_message_page(
        &self,
        _account_id: &AccountId,
        _mailbox_id: Option<&MailboxId>,
        _limit: usize,
        _cursor: Option<&MessageCursor>,
        _sort_field: MessageSortField,
        _sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        Ok(MessagePage {
            items: Vec::new(),
            next_cursor: None,
        })
    }
}

impl TagReadStore for TestStore {
    fn list_tags(&self, _account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError> {
        Ok(Vec::new())
    }
}

impl SmartMailboxStore for TestStore {
    fn query_messages_by_rule(
        &self,
        _rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        Ok(Vec::new())
    }

    fn query_message_page_by_rule(
        &self,
        _rule: &SmartMailboxRule,
        limit: usize,
        _cursor: Option<&MessageCursor>,
        _sort_field: MessageSortField,
        _sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        let items = self
            .rule_page
            .lock()
            .expect("rule page lock poisoned")
            .iter()
            .take(limit)
            .cloned()
            .collect();
        Ok(MessagePage {
            items,
            next_cursor: None,
        })
    }

    fn query_conversations_by_rule(
        &self,
        _rule: &SmartMailboxRule,
        _limit: usize,
        _cursor: Option<&ConversationCursor>,
        _sort_field: ConversationSortField,
        _sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        Ok(ConversationPage {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    fn query_smart_mailbox_counts(
        &self,
        _rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), StoreError> {
        self.smart_mailbox_counts_error
            .as_ref()
            .map_or(Ok((1, 2)), |error| Err(StoreError::Failure(error.clone())))
    }
}

impl ConversationReadStore for TestStore {
    fn list_conversations(
        &self,
        _account_id: Option<&AccountId>,
        _mailbox_id: Option<&MailboxId>,
        _limit: usize,
        _cursor: Option<&ConversationCursor>,
        _sort_field: ConversationSortField,
        _sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        Ok(ConversationPage {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    fn get_conversation(
        &self,
        _conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError> {
        Ok(None)
    }
}

impl MessageDetailStore for TestStore {
    fn get_message_detail(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        Ok(None)
    }

    fn get_thread(
        &self,
        _account_id: &AccountId,
        _thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError> {
        Ok(None)
    }
}

impl SyncStateStore for TestStore {
    fn get_sync_cursors(&self, _account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError> {
        Ok(self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned")
            .cursor
            .clone()
            .into_iter()
            .collect())
    }

    fn get_cursor(
        &self,
        _account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError> {
        if object_type == SyncObject::Message {
            return Ok(self
                .mutation_state
                .lock()
                .expect("mutation state lock poisoned")
                .cursor
                .clone());
        }
        Ok(None)
    }
}

impl MessageMailboxStore for TestStore {
    fn get_message_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        Ok(self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned")
            .mailbox_ids
            .clone())
    }
}

impl ImapSyncStateStore for TestStore {
    fn list_imap_mailbox_states(
        &self,
        _account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError> {
        Ok(Vec::new())
    }

    fn get_imap_mailbox_state(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError> {
        Ok(None)
    }
}

impl ImapMessageLocationStore for TestStore {
    fn list_imap_message_locations(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        Ok(Vec::new())
    }

    fn list_imap_mailbox_message_locations(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        Ok(Vec::new())
    }
}

impl SyncWriteStore for TestStore {
    fn apply_sync_batch(
        &self,
        _account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        if let Some(cursor) = batch
            .cursors
            .iter()
            .find(|cursor| cursor.object_type == SyncObject::Message)
        {
            state.cursor = Some(cursor.clone());
        }
        if let Some(message) = batch.messages.last() {
            state.mailbox_ids = message.mailbox_ids.clone();
        }
        if !batch.deleted_message_ids.is_empty() {
            state.mailbox_ids.clear();
        }
        Ok(Vec::new())
    }

    fn apply_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError> {
        if let Some(error) = &self.apply_body_error {
            return Err(StoreError::Failure(error.clone()));
        }
        self.applied_bodies
            .lock()
            .expect("applied bodies lock poisoned")
            .push((
                message_id.clone(),
                body.body_html.clone(),
                body.body_text.clone(),
            ));
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }
}

impl crate::CacheStore for TestStore {
    fn upsert_cache_candidates(
        &self,
        candidates: &[crate::CacheCandidate],
    ) -> Result<(), StoreError> {
        self.cache_candidates
            .lock()
            .expect("cache candidates lock poisoned")
            .extend(candidates.iter().cloned());
        Ok(())
    }

    fn record_cache_signal_updates(
        &self,
        updates: &[crate::CacheSignalUpdate],
    ) -> Result<(), StoreError> {
        self.cache_signal_updates
            .lock()
            .expect("cache signal updates lock poisoned")
            .extend(updates.iter().cloned());
        Ok(())
    }

    fn list_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        limit: usize,
    ) -> Result<Vec<crate::CacheRescoreCandidate>, StoreError> {
        Ok(self
            .cache_rescore_candidates
            .lock()
            .expect("cache rescore candidates lock poisoned")
            .iter()
            .filter(|candidate| candidate.account_id == account_id.as_str())
            .take(limit)
            .cloned()
            .collect())
    }

    fn queue_stale_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        stale_before: &str,
        limit: usize,
    ) -> Result<usize, StoreError> {
        self.stale_cache_rescore_requests
            .lock()
            .expect("stale cache rescore requests lock poisoned")
            .push((account_id.clone(), stale_before.to_string(), limit));
        Ok(self.stale_cache_rescore_result)
    }

    fn update_cache_priorities(
        &self,
        updates: &[crate::CachePriorityUpdate],
    ) -> Result<(), StoreError> {
        self.cache_priority_updates
            .lock()
            .expect("cache priority updates lock poisoned")
            .extend(updates.iter().cloned());
        Ok(())
    }

    fn list_cache_fetch_candidates(
        &self,
        account_id: &AccountId,
        layer: crate::CacheLayer,
        limit: usize,
    ) -> Result<Vec<crate::CacheFetchCandidate>, StoreError> {
        Ok(self
            .cache_fetch_candidates
            .lock()
            .expect("cache fetch candidates lock poisoned")
            .iter()
            .filter(|candidate| {
                candidate.account_id == account_id.as_str() && candidate.layer == layer
            })
            .take(limit)
            .cloned()
            .collect())
    }

    fn mark_cache_object_state(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        _layer: crate::CacheLayer,
        _object_id: Option<&str>,
        state: crate::CacheObjectState,
        error_code: Option<&str>,
    ) -> Result<(), StoreError> {
        self.cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned")
            .push((
                message_id.clone(),
                state,
                error_code.map(ToString::to_string),
            ));
        Ok(())
    }

    fn cache_used_bytes(&self) -> Result<u64, StoreError> {
        Ok(*self
            .cache_used_bytes
            .lock()
            .expect("cache used bytes lock poisoned"))
    }
}

impl MessageCommandStore for TestStore {
    fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError> {
        self.keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .push((message_id.clone(), command.add.clone()));
        if let Some(cursor) = cursor {
            self.mutation_state
                .lock()
                .expect("mutation state lock poisoned")
                .cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }

    fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError> {
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.mailbox_ids = command.mailbox_ids.clone();
        if let Some(cursor) = cursor {
            state.cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }

    fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError> {
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.mailbox_ids.clear();
        if let Some(cursor) = cursor {
            state.cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }
}

impl EventStore for TestStore {
    fn list_events(&self, _filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
        Ok(Vec::new())
    }

    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: serde_json::Value,
    ) -> Result<DomainEvent, StoreError> {
        Ok(DomainEvent {
            seq: 1,
            account_id: account_id.clone(),
            topic: topic.to_string(),
            occurred_at: crate::RFC3339_EPOCH.to_string(),
            mailbox_id: mailbox_id.cloned(),
            message_id: message_id.cloned(),
            payload,
        })
    }
}

impl SourceProjectionStore for TestStore {
    fn upsert_source_projection(
        &self,
        source_id: &AccountId,
        _name: &str,
    ) -> Result<(), StoreError> {
        self.projection_calls
            .lock()
            .expect("projection lock poisoned")
            .push(source_id.to_string());
        Ok(())
    }

    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError> {
        self.projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .push(source_id.to_string());
        Ok(())
    }
}

impl SourceDataStore for TestStore {
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError> {
        self.source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .push(account_id.to_string());
        Ok(())
    }
}

impl SenderAddressCacheStore for TestStore {
    fn list_sender_address_cache(&self) -> Result<Vec<CachedSenderAddress>, StoreError> {
        Ok(Vec::new())
    }

    fn remember_sender_address(
        &self,
        _account_id: &AccountId,
        _sender: &Recipient,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl AutomationBackfillStore for TestStore {
    fn ensure_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            return Ok(job.clone());
        }
        let job = AutomationBackfillJob {
            account_id: account_id.clone(),
            rule_fingerprint: rule_fingerprint.to_string(),
            status: AutomationBackfillJobStatus::Pending,
            attempts: 0,
            last_error: None,
            updated_at: crate::RFC3339_EPOCH.to_string(),
        };
        jobs.push(job.clone());
        Ok(job)
    }

    fn complete_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<(), StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            job.status = AutomationBackfillJobStatus::Completed;
            job.last_error = None;
        }
        Ok(())
    }

    fn record_automation_backfill_failure(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            job.status = AutomationBackfillJobStatus::Pending;
            job.attempts += 1;
            job.last_error = Some(error.to_string());
        }
        Ok(())
    }

    fn get_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<Option<AutomationBackfillJob>, StoreError> {
        Ok(self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned")
            .iter()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
            .cloned())
    }
}

fn sample_smart_mailbox() -> SmartMailbox {
    SmartMailbox {
        id: SmartMailboxId::from("default-inbox"),
        name: "Inbox".to_string(),
        position: 0,
        kind: SmartMailboxKind::Default,
        default_key: Some("inbox".to_string()),
        parent_id: None,
        rule: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::MailboxRole,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("inbox".to_string()),
                })],
            },
        },
        created_at: crate::RFC3339_EPOCH.to_string(),
        updated_at: crate::RFC3339_EPOCH.to_string(),
    }
}

fn sample_source() -> AccountSettings {
    AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: crate::AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: Default::default(),
        created_at: crate::RFC3339_EPOCH.to_string(),
        updated_at: crate::RFC3339_EPOCH.to_string(),
    }
}

fn sample_message_summary(id: &str, keywords: Vec<String>) -> MessageSummary {
    MessageSummary {
        id: MessageId::from(id),
        source_id: AccountId::from("primary"),
        source_name: "Primary".to_string(),
        source_thread_id: ThreadId::from("thread-1"),
        conversation_id: ConversationId::from("conversation-1"),
        subject: Some("Hello".to_string()),
        from_name: Some("PostHaste Updates".to_string()),
        from_email: Some("hello@example.com".to_string()),
        to: Vec::new(),
        preview: None,
        received_at: crate::RFC3339_EPOCH.to_string(),
        has_attachment: false,
        is_read: false,
        is_flagged: false,
        mailbox_ids: vec![MailboxId::from("inbox")],
        keywords,
    }
}

fn sample_message_record(id: &str, size: i64, has_attachment: bool) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from("thread-1"),
        remote_blob_id: None,
        subject: Some("Hello".to_string()),
        from_name: Some("PostHaste Updates".to_string()),
        from_email: Some("hello@example.com".to_string()),
        to: Vec::new(),
        preview: None,
        received_at: crate::RFC3339_EPOCH.to_string(),
        has_attachment,
        size,
        mailbox_ids: vec![MailboxId::from("inbox")],
        keywords: Vec::new(),
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id: None,
        in_reply_to: None,
        references: Vec::new(),
    }
}

fn sample_cache_fetch_candidate(message_id: &str, fetch_bytes: u64) -> CacheFetchCandidate {
    CacheFetchCandidate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        fetch_bytes,
        priority: 1.0,
    }
}

fn sample_fetch_lease(request_limit: usize, byte_limit: u64) -> CacheFetchLease {
    CacheFetchLease::new(request_limit, byte_limit, 0.0)
}

fn sample_cache_rescore_candidate(message_id: &str) -> CacheRescoreCandidate {
    CacheRescoreCandidate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        state: CacheObjectState::Wanted,
        value_bytes: 32 * 1024,
        fetch_bytes: 32 * 1024,
        priority: 1.0,
        message_size: 32 * 1024,
        has_attachment: false,
        received_at: crate::RFC3339_EPOCH.to_string(),
        in_inbox: true,
        unread: true,
        flagged: false,
        thread_activity: 0.0,
        sender_affinity: 0.0,
        local_behavior: 0.0,
        search: Some(crate::CacheSearchSignals {
            total_messages: 1_000,
            result_count: 5,
            result_rank: 0,
        }),
        direct_user_boost: 0.8,
        pinned: false,
        signal_reason: "search-visible".to_string(),
        rescore_priority: 108.0,
    }
}

fn sample_fetched_body() -> FetchedBody {
    FetchedBody {
        body_html: None,
        body_text: Some("Cached body".to_string()),
        raw_mime: None,
        attachments: Vec::new(),
    }
}

fn sample_automation_rule() -> AutomationRule {
    AutomationRule {
        id: "rule-posthaste".to_string(),
        name: "Posthaste".to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::Any,
                negated: false,
                nodes: vec![
                    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::FromName,
                        operator: SmartMailboxOperator::Contains,
                        negated: false,
                        value: SmartMailboxValue::String("posthaste".to_string()),
                    }),
                    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::FromEmail,
                        operator: SmartMailboxOperator::Contains,
                        negated: false,
                        value: SmartMailboxValue::String("posthaste".to_string()),
                    }),
                ],
            },
        },
        actions: vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
        backfill: true,
    }
}

struct MutationGateway {
    revision: Mutex<u64>,
    batch: Option<SyncBatch>,
    fetch_body_result: Mutex<Option<Result<FetchedBody, GatewayError>>>,
    fetch_attempts: Mutex<Vec<MessageId>>,
}

impl MutationGateway {
    fn with_revision(revision: u64) -> Self {
        Self {
            revision: Mutex::new(revision),
            batch: None,
            fetch_body_result: Mutex::new(None),
            fetch_attempts: Mutex::new(Vec::new()),
        }
    }

    fn with_sync_batch(revision: u64, batch: SyncBatch) -> Self {
        Self {
            revision: Mutex::new(revision),
            batch: Some(batch),
            fetch_body_result: Mutex::new(None),
            fetch_attempts: Mutex::new(Vec::new()),
        }
    }

    fn with_fetch_body_result(result: Result<FetchedBody, GatewayError>) -> Self {
        Self {
            revision: Mutex::new(1),
            batch: None,
            fetch_body_result: Mutex::new(Some(result)),
            fetch_attempts: Mutex::new(Vec::new()),
        }
    }

    fn apply(&self, expected_state: Option<&str>) -> Result<MutationOutcome, GatewayError> {
        let mut revision = self.revision.lock().expect("revision lock poisoned");
        if let Some(expected_state) = expected_state {
            let current = format!("message-{}", *revision);
            if expected_state != current {
                return Err(GatewayError::StateMismatch);
            }
        }
        *revision += 1;
        Ok(MutationOutcome {
            cursor: Some(SyncCursor {
                object_type: SyncObject::Message,
                state: format!("message-{}", *revision),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }),
        })
    }
}

#[async_trait]
impl MailGateway for MutationGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
        _progress: Option<crate::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        self.batch
            .clone()
            .ok_or_else(|| GatewayError::Rejected("unused".to_string()))
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        self.fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned")
            .push(message_id.clone());
        self.fetch_body_result
            .lock()
            .expect("fetch body result lock poisoned")
            .take()
            .unwrap_or_else(|| Err(GatewayError::Rejected("unused".to_string())))
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &crate::BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
        _command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        self.apply(expected_state)
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
        _mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        self.apply(expected_state)
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        self.apply(expected_state)
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

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<crate::ReplyContext, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        vec![]
    }
}

#[test]
fn list_smart_mailboxes_propagates_store_count_errors() {
    let store = Arc::new(TestStore {
        smart_mailbox_counts_error: Some("counts failed".to_string()),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        smart_mailboxes: vec![sample_smart_mailbox()],
        sources: Vec::new(),
        ..Default::default()
    });
    let service = MailService::new(store, config);

    let error = service
        .list_smart_mailboxes()
        .expect_err("count failures should not be swallowed");

    assert_eq!(error.code(), "storage_failure");
}

#[test]
fn get_sidebar_propagates_mailbox_listing_errors() {
    let store = Arc::new(TestStore {
        list_mailboxes_error: Some("mailboxes failed".to_string()),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        smart_mailboxes: vec![sample_smart_mailbox()],
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store, config);

    let error = service
        .get_sidebar()
        .expect_err("mailbox failures should not be swallowed");

    assert_eq!(error.code(), "storage_failure");
}

#[tokio::test]
async fn sync_account_records_body_cache_candidate_with_body_only_fetch_cost() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![sample_message_record("message-1", 12 * 1024 * 1024, true)],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should succeed");

    let candidates = store
        .cache_candidates
        .lock()
        .expect("cache candidates lock poisoned");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].layer, CacheLayer::Body);
    assert_eq!(candidates[0].fetch_unit, CacheFetchUnit::BodyOnly);
    assert_eq!(candidates[0].fetch_bytes, 256 * 1024);
}

#[tokio::test]
async fn sync_account_records_imap_body_candidate_with_raw_message_fetch_cost() {
    let mut account = sample_source();
    account.driver = AccountDriver::ImapSmtp;
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![sample_message_record("message-1", 12 * 1024 * 1024, true)],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should succeed");

    let candidates = store
        .cache_candidates
        .lock()
        .expect("cache candidates lock poisoned");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].layer, CacheLayer::Body);
    assert_eq!(candidates[0].fetch_unit, CacheFetchUnit::RawMessage);
    assert_eq!(candidates[0].value_bytes, 256 * 1024);
    assert_eq!(candidates[0].fetch_bytes, 12 * 1024 * 1024);
}

#[test]
fn search_visibility_records_ranked_cache_signal_updates() {
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let page = MessagePage {
        items: vec![
            sample_message_summary("message-1", Vec::new()),
            sample_message_summary("message-2", Vec::new()),
        ],
        next_cursor: None,
    };

    let account_ids = service
        .record_cache_search_visibility(&page, 100, 2)
        .expect("visibility recording should succeed");

    assert_eq!(account_ids, vec![AccountId::from("primary")]);
    let updates = store
        .cache_signal_updates
        .lock()
        .expect("cache signal updates lock poisoned");
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].reason, "search-visible");
    assert_eq!(updates[0].search.as_ref().unwrap().total_messages, 100);
    assert_eq!(updates[0].search.as_ref().unwrap().result_count, 2);
    assert_eq!(updates[0].search.as_ref().unwrap().result_rank, 0);
    assert_eq!(updates[1].search.as_ref().unwrap().result_rank, 1);
    assert!(updates[0].direct_user_boost.unwrap() > updates[1].direct_user_boost.unwrap());
}

#[test]
fn cache_rescore_batch_applies_search_signal_priority() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_rescore_candidates: Mutex::new(vec![sample_cache_rescore_candidate("message-1")]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let outcome = service
        .process_cache_rescore_batch(&account_id, 10)
        .expect("rescore should succeed");

    assert_eq!(outcome.scanned, 1);
    assert_eq!(outcome.updated, 1);
    let updates = store
        .cache_priority_updates
        .lock()
        .expect("cache priority updates lock poisoned");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].message_id, "message-1");
    assert_eq!(updates[0].reason, "search-visible");
    assert!(updates[0].priority > 1.0);
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn cache_rescore_batch_rebuilds_imap_body_fetch_cost_from_metadata() {
    let mut account = sample_source();
    account.driver = AccountDriver::ImapSmtp;
    let account_id = account.id.clone();
    let mut candidate = sample_cache_rescore_candidate("message-1");
    candidate.fetch_unit = CacheFetchUnit::BodyOnly;
    candidate.value_bytes = 0;
    candidate.fetch_bytes = 0;
    candidate.message_size = 12 * 1024 * 1024;
    candidate.has_attachment = true;
    let store = Arc::new(TestStore {
        cache_rescore_candidates: Mutex::new(vec![candidate]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let outcome = service
        .process_cache_rescore_batch(&account_id, 10)
        .expect("rescore should succeed");

    assert_eq!(outcome.updated, 1);
    let updates = store
        .cache_priority_updates
        .lock()
        .expect("cache priority updates lock poisoned");
    assert_eq!(updates[0].fetch_unit, CacheFetchUnit::RawMessage);
    assert_eq!(updates[0].value_bytes, 256 * 1024);
    assert_eq!(updates[0].fetch_bytes, 12 * 1024 * 1024);
}

// spec: docs/L1-sync#cache-stale-rescore
#[test]
fn stale_cache_rescore_batch_queues_bounded_cutoff() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        stale_cache_rescore_result: 7,
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);

    let queued = service
        .queue_stale_cache_rescore_batch(&account_id, Duration::from_secs(60), 25)
        .expect("stale queue should succeed");

    assert_eq!(queued, 7);
    let requests = store
        .stale_cache_rescore_requests
        .lock()
        .expect("stale cache rescore requests lock poisoned");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, account_id);
    assert_eq!(requests[0].2, 25);
    assert!(!requests[0].1.is_empty());
}

#[tokio::test]
async fn body_cache_worker_fetches_admitted_candidates_and_marks_cached() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should fetch an admitted body");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 32 * 1024);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.cached_bytes, 32 * 1024);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 0);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("message-1")]
    );
    assert_eq!(
        *store
            .applied_bodies
            .lock()
            .expect("applied bodies lock poisoned"),
        vec![(
            MessageId::from("message-1"),
            None,
            Some("Cached body".to_string())
        )]
    );
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (MessageId::from("message-1"), CacheObjectState::Cached, None),
        ]
    );
}

#[tokio::test]
async fn body_cache_worker_marks_gateway_failures_and_continues() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway =
        MutationGateway::with_fetch_body_result(Err(GatewayError::Network("offline".to_string())));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should record fetch failures");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 32 * 1024);
    assert_eq!(outcome.cached, 0);
    assert_eq!(outcome.cached_bytes, 0);
    assert_eq!(outcome.failed, 1);
    assert_eq!(outcome.skipped, 0);
    assert!(store
        .applied_bodies
        .lock()
        .expect("applied bodies lock poisoned")
        .is_empty());
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (
                MessageId::from("message-1"),
                CacheObjectState::Failed,
                Some("network_error".to_string())
            ),
        ]
    );
}

#[tokio::test]
async fn body_cache_worker_surfaces_store_failures() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        apply_body_error: Some("write failed".to_string()),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let error = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect_err("cache worker should surface local store failures");

    assert_eq!(error.code(), "storage_failure");
    assert_eq!(
        *store
            .cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned"),
        vec![
            (
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            ),
            (
                MessageId::from("message-1"),
                CacheObjectState::Failed,
                Some("storage_failure".to_string())
            )
        ]
    );
}

#[tokio::test]
async fn body_cache_worker_skips_candidates_that_do_not_fit_budget() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![sample_cache_fetch_candidate(
            "message-1",
            32 * 1024,
        )]),
        cache_used_bytes: Mutex::new(1024),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        app_settings: Mutex::new(AppSettings {
            cache_policy: CachePolicy {
                soft_cap_bytes: 1024,
                hard_cap_bytes: 1024,
                cache_bodies: true,
                cache_raw_messages: false,
                cache_attachments: false,
            },
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(10, 1024 * 1024))
        .await
        .expect("cache worker should skip over-budget candidates");

    assert_eq!(outcome.attempted, 0);
    assert_eq!(outcome.cached, 0);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 1);
    assert!(gateway
        .fetch_attempts
        .lock()
        .expect("fetch attempts lock poisoned")
        .is_empty());
    assert!(store
        .cache_state_changes
        .lock()
        .expect("cache state changes lock poisoned")
        .is_empty());
}

#[tokio::test]
async fn body_cache_worker_scans_past_large_candidates_to_find_one_that_fits() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("too-large", 2 * 1024),
            sample_cache_fetch_candidate("small-enough", 512),
        ]),
        cache_used_bytes: Mutex::new(1024),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        app_settings: Mutex::new(AppSettings {
            cache_policy: CachePolicy {
                soft_cap_bytes: 2 * 1024,
                hard_cap_bytes: 2 * 1024,
                cache_bodies: true,
                cache_raw_messages: false,
                cache_attachments: false,
            },
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(1, 1024 * 1024))
        .await
        .expect("cache worker should scan past oversized candidates");

    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("small-enough")]
    );
}

#[tokio::test]
async fn body_cache_worker_respects_fetch_byte_lease() {
    let account_id = AccountId::from("primary");
    let store = Arc::new(TestStore {
        cache_fetch_candidates: Mutex::new(vec![
            sample_cache_fetch_candidate("too-large-for-lease", 2 * 1024),
            sample_cache_fetch_candidate("fits-lease", 512),
        ]),
        ..Default::default()
    });
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_fetch_body_result(Ok(sample_fetched_body()));

    let outcome = service
        .process_body_cache_batch(&account_id, &gateway, sample_fetch_lease(2, 1024))
        .await
        .expect("cache worker should respect fetch byte lease");

    assert_eq!(outcome.scanned, 2);
    assert_eq!(outcome.attempted, 1);
    assert_eq!(outcome.attempted_bytes, 512);
    assert_eq!(outcome.cached, 1);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        *gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned"),
        vec![MessageId::from("fits-lease")]
    );
}

#[tokio::test]
async fn consecutive_keyword_mutations_advance_message_cursor() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);

    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect("flagging should succeed");
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-2"
    );

    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: Vec::new(),
                remove: vec!["$flagged".to_string()],
            },
            &gateway,
        )
        .await
        .expect("unflagging should succeed");
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-3"
    );
}

#[tokio::test]
async fn sync_applies_matching_automation_tag() {
    let account_id = AccountId::from("primary");
    let account = sample_source();
    let store = Arc::new(TestStore::default());
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let config = Arc::new(TestConfig {
        sources: vec![account],
        app_settings: Mutex::new(AppSettings {
            default_account_id: None,
            automation_rules: vec![sample_automation_rule()],
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![MessageRecord {
                id: MessageId::from("message-1"),
                source_thread_id: ThreadId::from("thread-1"),
                remote_blob_id: None,
                subject: Some("Welcome".to_string()),
                from_name: Some("PostHaste Updates".to_string()),
                from_email: Some("hello@example.com".to_string()),
                to: Vec::new(),
                preview: None,
                received_at: crate::RFC3339_EPOCH.to_string(),
                has_attachment: false,
                size: 0,
                mailbox_ids: vec![MailboxId::from("inbox")],
                keywords: Vec::new(),
                body_html: None,
                body_text: None,
                raw_mime: None,
                rfc_message_id: None,
                in_reply_to: None,
                references: Vec::new(),
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should apply action");

    assert_eq!(
        *store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned"),
        vec![(MessageId::from("message-1"), vec!["newsletter".to_string()])]
    );
}

#[tokio::test]
async fn automation_backfill_processes_one_bounded_batch() {
    let account_id = AccountId::from("primary");
    let account = sample_source();
    let store = Arc::new(TestStore::default());
    *store.rule_page.lock().expect("rule page lock poisoned") = vec![
        sample_message_summary("message-1", Vec::new()),
        sample_message_summary("message-2", Vec::new()),
    ];
    let config = Arc::new(TestConfig {
        sources: vec![account],
        app_settings: Mutex::new(AppSettings {
            default_account_id: None,
            automation_rules: vec![sample_automation_rule()],
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);

    let (_events, has_more) = service
        .backfill_automation_rules_batch(&account_id, &gateway, 1)
        .await
        .expect("backfill should apply one bounded batch");

    assert!(has_more);
    assert_eq!(
        *store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned"),
        vec![(MessageId::from("message-1"), vec!["newsletter".to_string()])]
    );
}

#[tokio::test]
async fn mixed_message_mutations_reuse_advanced_cursor() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);

    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect("first mutation should succeed");
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
            &gateway,
        )
        .await
        .expect("second mutation should succeed");

    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-3"
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")]
    );
}

// spec: docs/L0-testing#sync-convergence-contracts
// spec: docs/L1-sync#conflict-model
#[tokio::test]
async fn state_mismatch_refreshes_remote_projection_without_retrying_original_mutation() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let mut remote_message = sample_message_record("message-1", 0, false);
    remote_message.mailbox_ids = vec![MailboxId::from("archive")];
    let gateway = MutationGateway::with_sync_batch(
        2,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![remote_message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "message-2".to_string(),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }],
        },
    );

    let error = service
        .set_keywords(
            &account_id,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect_err("stale mutation should still report a state mismatch");

    assert_eq!(error.code(), "state_mismatch");
    assert_eq!(
        store
            .get_cursor(&account_id, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account_id, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")]
    );
    assert!(store
        .keyword_adds
        .lock()
        .expect("keyword adds lock poisoned")
        .is_empty());
}

#[tokio::test]
async fn genuine_state_mismatch_is_not_retried() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        2,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "message-2".to_string(),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }],
        },
    );

    let error = service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect_err("mismatch should be returned to the caller");

    assert_eq!(error.code(), "state_mismatch");
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    assert!(store
        .keyword_adds
        .lock()
        .expect("keyword adds lock poisoned")
        .is_empty());
}

#[test]
fn delete_source_clears_default_account_before_removing_it() {
    let account = sample_source();
    let config = Arc::new(TestConfig {
        sources: vec![account.clone()],
        app_settings: Mutex::new(AppSettings {
            default_account_id: Some(account.id.clone()),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), config.clone());

    service
        .delete_source(&account.id)
        .expect("deleting the account should succeed");

    assert_eq!(
        config
            .get_app_settings()
            .expect("settings lookup should succeed")
            .default_account_id,
        None
    );
    assert_eq!(
        config
            .deleted_sources
            .lock()
            .expect("deleted sources lock poisoned")
            .as_slice(),
        std::slice::from_ref(&account.id)
    );
    assert_eq!(
        store
            .projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .as_slice(),
        &[account.id.to_string()]
    );
    assert_eq!(
        store
            .source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .as_slice(),
        &[account.id.to_string()]
    );
}

#[test]
fn reload_config_cleans_up_removed_sources_before_resyncing_projections() {
    let removed = AccountId::from("removed");
    let remaining = sample_source();
    let config = Arc::new(TestConfig {
        sources: vec![remaining.clone()],
        reload_diff: ConfigDiff {
            added_sources: Vec::new(),
            changed_sources: Vec::new(),
            removed_sources: vec![removed.clone()],
        },
        ..Default::default()
    });
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), config);

    let diff = service
        .reload_config()
        .expect("reloading config should succeed");

    assert_eq!(diff.removed_sources, vec![removed.clone()]);
    assert_eq!(
        store
            .projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .as_slice(),
        &[removed.to_string()]
    );
    assert_eq!(
        store
            .source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .as_slice(),
        &[removed.to_string()]
    );
    assert_eq!(
        store
            .projection_calls
            .lock()
            .expect("projection lock poisoned")
            .as_slice(),
        &[remaining.id.to_string()]
    );
}
