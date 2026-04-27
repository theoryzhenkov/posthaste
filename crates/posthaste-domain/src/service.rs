use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tracing::{debug, trace};

use crate::{
    decide_cache_admission, score_cache_candidate, AccountDriver, AccountId, AccountSettings,
    AddToMailboxCommand, AppSettings, AutomationAction, AutomationBackfillBatchOutcome,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationBackfillStore, AutomationRule,
    AutomationTrigger, CacheAdmission, CacheCandidate, CacheCandidateSignals, CacheFetchLease,
    CacheFetchUnit, CacheLayer, CacheMessageSignals, CacheObjectState, CachePolicy,
    CachePriorityUpdate, CacheRescoreBatchOutcome, CacheRescoreCandidate, CacheSignalUpdate,
    CacheStore, CacheWorkerBatchOutcome, CommandResult, ConfigDiff, ConfigRepository,
    ConversationCursor, ConversationId, ConversationPage, ConversationReadStore,
    ConversationSortField, ConversationView, EventStore, Identity, MailGateway, MailStore,
    MailboxId, MailboxReadStore, MailboxSummary, MessageCommandStore, MessageCursor,
    MessageDetailStore, MessageId, MessageListStore, MessageMailboxStore, MessagePage,
    MessageRecord, MessageSortField, MessageSummary, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, SendMessageRequest, ServiceError, SetKeywordsCommand,
    SharedConfigRepository, SidebarResponse, SidebarSmartMailbox, SidebarSource, SmartMailbox,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxId, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxStore, SmartMailboxSummary, SmartMailboxValue, SortDirection, SourceDataStore,
    SourceProjectionStore, StoreError, SyncObject, SyncStateStore, SyncTrigger, SyncWriteStore,
    TagReadStore, TagSummary, ThreadId, ThreadView, EVENT_TOPIC_SYNC_COMPLETED,
    EVENT_TOPIC_SYNC_FAILED,
};
use crate::{DomainEvent, ServiceResultExt};

/// Internal enum dispatching message mutations through a shared code path.
#[derive(Clone, Copy)]
enum MessageMutation<'a> {
    SetKeywords(&'a SetKeywordsCommand),
    ReplaceMailboxes(&'a ReplaceMailboxesCommand),
    Destroy,
}

fn condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

fn negated_condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: true,
        value,
    })
}

fn automation_query_rule(
    account_id: &AccountId,
    rule: &AutomationRule,
    action: &AutomationAction,
    message_ids: &[MessageId],
) -> SmartMailboxRule {
    let mut nodes = vec![
        condition_node(
            SmartMailboxField::SourceId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(account_id.to_string()),
        ),
        SmartMailboxRuleNode::Group(rule.condition.root.clone()),
    ];

    if !message_ids.is_empty() {
        nodes.push(condition_node(
            SmartMailboxField::MessageId,
            SmartMailboxOperator::In,
            SmartMailboxValue::Strings(message_ids.iter().map(ToString::to_string).collect()),
        ));
    }

    if let Some(precondition) = automation_action_precondition(action) {
        nodes.push(precondition);
    }

    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn automation_action_precondition(action: &AutomationAction) -> Option<SmartMailboxRuleNode> {
    match action {
        AutomationAction::ApplyTag { tag } => Some(negated_condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::RemoveTag { tag } => Some(condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::MarkRead => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::MarkUnread => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::Flag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::Unflag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::MoveToMailbox { mailbox_id } => Some(negated_condition_node(
            SmartMailboxField::MailboxId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(mailbox_id.to_string()),
        )),
    }
}

fn automation_backfill_fingerprint(settings: &AppSettings) -> Result<Option<String>, ServiceError> {
    let rules = settings
        .automation_rules
        .iter()
        .filter(|rule| rule.enabled && rule.backfill)
        .cloned()
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&rules).map(Some).map_err(|err| {
        StoreError::Failure(format!("failed to fingerprint automation rules: {err}")).into()
    })
}

fn message_age_days(received_at: &str) -> f64 {
    let Ok(received_at) =
        time::OffsetDateTime::parse(received_at, &time::format_description::well_known::Rfc3339)
    else {
        return 365.0;
    };
    let now = time::OffsetDateTime::now_utc();
    let seconds = (now - received_at).whole_seconds().max(0) as f64;
    seconds / 86_400.0
}

fn nonnegative_message_size(size: i64) -> u64 {
    u64::try_from(size.max(0)).unwrap_or(0)
}

fn estimated_body_bytes(message: &MessageRecord) -> u64 {
    estimated_body_bytes_from_metadata(message.size, message.has_attachment)
}

fn estimated_body_bytes_from_metadata(size: i64, has_attachment: bool) -> u64 {
    let metadata_size = nonnegative_message_size(size);
    if metadata_size == 0 {
        return 64 * 1024;
    }
    if has_attachment {
        metadata_size.min(256 * 1024).max(16 * 1024)
    } else {
        metadata_size.max(4 * 1024)
    }
}

fn body_fetch_unit(account: &AccountSettings) -> CacheFetchUnit {
    match account.driver {
        AccountDriver::ImapSmtp => CacheFetchUnit::RawMessage,
        AccountDriver::Jmap | AccountDriver::Mock => CacheFetchUnit::BodyOnly,
    }
}

fn body_fetch_bytes(account: &AccountSettings, message: &MessageRecord) -> u64 {
    body_fetch_bytes_from_metadata(account, message.size, message.has_attachment)
}

fn body_fetch_bytes_from_metadata(
    account: &AccountSettings,
    size: i64,
    has_attachment: bool,
) -> u64 {
    match body_fetch_unit(account) {
        CacheFetchUnit::RawMessage => nonnegative_message_size(size).max(4 * 1024),
        CacheFetchUnit::BodyOnly => estimated_body_bytes_from_metadata(size, has_attachment),
        CacheFetchUnit::AttachmentBlob => unreachable!("body cache never fetches attachment blobs"),
    }
}

fn visible_rank_direct_boost(rank: u64) -> f64 {
    0.8 / ((rank + 1) as f64).sqrt()
}

fn rescore_candidate_signals(
    candidate: &CacheRescoreCandidate,
    fetch_unit: CacheFetchUnit,
    value_bytes: u64,
    fetch_bytes: u64,
) -> CacheCandidateSignals {
    CacheCandidateSignals {
        message: CacheMessageSignals {
            age_days: message_age_days(&candidate.received_at),
            in_inbox: candidate.in_inbox,
            unread: candidate.unread,
            flagged: candidate.flagged,
            thread_activity: candidate.thread_activity,
            sender_affinity: candidate.sender_affinity,
            local_behavior: candidate.local_behavior,
            search: candidate.search.clone(),
        },
        layer: candidate.layer,
        fetch_unit,
        value_bytes,
        fetch_bytes,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: candidate.direct_user_boost,
        pinned: candidate.pinned,
    }
}

/// Orchestrates domain logic by composing gateway, store, and config ports.
///
/// `MailService` is the primary entry point for all business operations.
/// It owns no I/O or live connection registry -- external interactions flow
/// through explicit trait objects supplied by the application layer.
///
/// @spec docs/L0-api#rust-owns-everything
pub struct MailService {
    config: SharedConfigRepository,
    mailbox_reader: Arc<dyn MailboxReadStore>,
    message_lister: Arc<dyn MessageListStore>,
    tag_reader: Arc<dyn TagReadStore>,
    conversation_reader: Arc<dyn ConversationReadStore>,
    message_detail_reader: Arc<dyn MessageDetailStore>,
    smart_mailboxes: Arc<dyn SmartMailboxStore>,
    sync_state: Arc<dyn SyncStateStore>,
    message_mailboxes: Arc<dyn MessageMailboxStore>,
    sync_writer: Arc<dyn SyncWriteStore>,
    message_commands: Arc<dyn MessageCommandStore>,
    events: Arc<dyn EventStore>,
    source_projections: Arc<dyn SourceProjectionStore>,
    source_data: Arc<dyn SourceDataStore>,
    cache_store: Arc<dyn CacheStore>,
    automation_backfills: Arc<dyn AutomationBackfillStore>,
}

impl MailService {
    /// Create a new service with the given store and config repository.
    pub fn new<T>(store: Arc<T>, config: Arc<dyn ConfigRepository>) -> Self
    where
        T: MailStore + 'static,
    {
        Self {
            config,
            mailbox_reader: store.clone(),
            message_lister: store.clone(),
            tag_reader: store.clone(),
            conversation_reader: store.clone(),
            message_detail_reader: store.clone(),
            smart_mailboxes: store.clone(),
            sync_state: store.clone(),
            message_mailboxes: store.clone(),
            sync_writer: store.clone(),
            message_commands: store.clone(),
            events: store.clone(),
            source_projections: store.clone(),
            source_data: store.clone(),
            cache_store: store.clone(),
            automation_backfills: store,
        }
    }

    // -- Config delegates --

    /// Read global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn get_app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.config.get_app_settings().map_err(Into::into)
    }

    /// Persist updated global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ServiceError> {
        self.config.put_app_settings(settings).map_err(Into::into)
    }

    /// Ensure enabled accounts have a durable job for the current backfill rules.
    ///
    /// Completed jobs are preserved, so calling this on startup or after a
    /// settings PATCH is cheap unless the rule fingerprint changed.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub fn ensure_automation_backfills_for_current_rules(
        &self,
    ) -> Result<Vec<AutomationBackfillJob>, ServiceError> {
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(Vec::new());
        };
        self.config
            .list_sources()?
            .into_iter()
            .filter(|source| source.enabled)
            .map(|source| {
                self.automation_backfills
                    .ensure_automation_backfill_job(&source.id, &rule_fingerprint)
                    .map_err(Into::into)
            })
            .collect()
    }

    /// Return the current-rules backfill job for an account, if applicable.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub fn automation_backfill_job_for_current_rules(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<AutomationBackfillJob>, ServiceError> {
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(None);
        };
        self.automation_backfills
            .get_automation_backfill_job(account_id, &rule_fingerprint)
            .map_err(Into::into)
    }

    /// List all account configurations.
    ///
    /// @spec docs/L1-api#accounts
    pub fn list_sources(&self) -> Result<Vec<AccountSettings>, ServiceError> {
        self.config.list_sources().map_err(Into::into)
    }

    /// Look up a single account configuration by ID.
    pub fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ServiceError> {
        self.config.get_source(id).map_err(Into::into)
    }

    /// Create or update an account, syncing the source projection in the store.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn save_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.save_source(source)?;
        self.source_projections
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    /// Delete an account: remove config, projection, and all synced data.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn delete_source(&self, id: &AccountId) -> Result<(), ServiceError> {
        let mut settings = self.config.get_app_settings()?;
        if settings.default_account_id.as_ref() == Some(id) {
            settings.default_account_id = None;
            self.config.put_app_settings(&settings)?;
        }
        self.config.delete_source(id)?;
        self.source_projections.delete_source_projection(id)?;
        self.source_data.delete_source_data(id)?;
        Ok(())
    }

    /// List smart mailbox configurations (without live counts).
    pub fn list_smart_mailboxes_config(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config.list_smart_mailboxes().map_err(Into::into)
    }

    /// Fetch a single smart mailbox configuration, or 404.
    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())
    }

    /// Create or update a smart mailbox configuration.
    ///
    /// @spec docs/L1-api#smart-mailbox-crud
    pub fn save_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), ServiceError> {
        self.config
            .save_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    /// Delete a smart mailbox configuration.
    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<(), ServiceError> {
        self.config
            .delete_smart_mailbox(smart_mailbox_id)
            .map_err(Into::into)
    }

    /// Restore all default smart mailboxes, preserving user-created ones.
    ///
    /// @spec docs/L1-accounts#smart-mailbox-defaults
    pub fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    /// Re-read config from disk, diff it, and sync source projections.
    ///
    /// @spec docs/L1-accounts#configdiff
    pub fn reload_config(&self) -> Result<ConfigDiff, ServiceError> {
        let diff = self.config.reload()?;
        for source_id in &diff.removed_sources {
            self.source_projections
                .delete_source_projection(source_id)?;
            self.source_data.delete_source_data(source_id)?;
        }
        // Sync all source projections after reload
        self.sync_source_projections()?;
        Ok(diff)
    }

    /// Upsert source projection rows for all configured accounts.
    pub fn sync_source_projections(&self) -> Result<(), ServiceError> {
        let sources = self.config.list_sources()?;
        for source in &sources {
            self.source_projections
                .upsert_source_projection(&source.id, &source.name)?;
        }
        Ok(())
    }

    // -- Composed queries (config + store) --

    /// List smart mailboxes with live unread/total counts from the store.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        let mailboxes = self.config.list_smart_mailboxes()?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            let (unread, total) = self
                .smart_mailboxes
                .query_smart_mailbox_counts(&mailbox.rule)?;
            summaries.push(SmartMailboxSummary {
                id: mailbox.id,
                name: mailbox.name,
                position: mailbox.position,
                kind: mailbox.kind,
                default_key: mailbox.default_key,
                parent_id: mailbox.parent_id,
                unread_messages: unread,
                total_messages: total,
                created_at: mailbox.created_at,
                updated_at: mailbox.updated_at,
            });
        }
        Ok(summaries)
    }

    /// List messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_messages_by_rule(&mailbox.rule)
            .map_err(Into::into)
    }

    /// Paginated messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_message_page(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_message_page_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// List messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.smart_mailboxes
            .query_messages_by_rule(rule)
            .map_err(Into::into)
    }

    /// Count messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn count_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), ServiceError> {
        self.smart_mailboxes
            .query_smart_mailbox_counts(rule)
            .map_err(Into::into)
    }

    /// Paginated messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        self.smart_mailboxes
            .query_message_page_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Record visible search results as cache utility signals.
    ///
    /// This only updates local signal state and the re-score queue; the account
    /// runtime performs remote fetches asynchronously.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn record_cache_search_visibility(
        &self,
        page: &MessagePage,
        total_messages: u64,
        result_count: u64,
    ) -> Result<Vec<AccountId>, ServiceError> {
        if page.items.is_empty() {
            return Ok(Vec::new());
        }
        let total_messages = total_messages
            .max(result_count)
            .max(page.items.len() as u64);
        let result_count = result_count.max(page.items.len() as u64);
        let updates = page
            .items
            .iter()
            .enumerate()
            .map(|(rank, message)| CacheSignalUpdate {
                account_id: message.source_id.to_string(),
                message_id: message.id.to_string(),
                reason: "search-visible".to_string(),
                search: Some(crate::CacheSearchSignals {
                    total_messages,
                    result_count,
                    result_rank: rank as u64,
                }),
                thread_activity: None,
                sender_affinity: None,
                local_behavior: None,
                direct_user_boost: Some(visible_rank_direct_boost(rank as u64)),
                pinned: None,
            })
            .collect::<Vec<_>>();
        self.cache_store.record_cache_signal_updates(&updates)?;
        let account_ids = updates
            .iter()
            .map(|update| AccountId::from(update.account_id.as_str()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        debug!(
            message_count = updates.len(),
            account_count = account_ids.len(),
            total_messages,
            result_count,
            "cache search visibility signals recorded"
        );
        Ok(account_ids)
    }

    /// Paginated conversations matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_conversations(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_conversations_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Query conversations matching an arbitrary rule (used by search).
    pub fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        self.smart_mailboxes
            .query_conversations_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Build the full sidebar: smart mailboxes with counts + per-source mailboxes.
    ///
    /// @spec docs/L1-api#navigation
    pub fn get_sidebar(&self) -> Result<SidebarResponse, ServiceError> {
        let smart_mailboxes = self.config.list_smart_mailboxes()?;
        let sources = self.config.list_sources()?;

        let sidebar_smart_mailboxes: Vec<SidebarSmartMailbox> = smart_mailboxes
            .into_iter()
            .map(|mailbox| -> Result<SidebarSmartMailbox, ServiceError> {
                let (unread, total) = self
                    .smart_mailboxes
                    .query_smart_mailbox_counts(&mailbox.rule)?;
                Ok(SidebarSmartMailbox {
                    id: mailbox.id,
                    name: mailbox.name,
                    unread_messages: unread,
                    total_messages: total,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let sidebar_sources: Vec<SidebarSource> = sources
            .into_iter()
            .filter(|source| source.enabled)
            .map(|source| -> Result<SidebarSource, ServiceError> {
                let mailboxes = self.mailbox_reader.list_mailboxes(&source.id)?;
                Ok(SidebarSource {
                    id: source.id,
                    name: source.name,
                    mailboxes,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut tag_totals = std::collections::BTreeMap::<String, (i64, i64)>::new();
        for source in &sidebar_sources {
            for tag in self.tag_reader.list_tags(&source.id)? {
                let entry = tag_totals.entry(tag.name).or_insert((0, 0));
                entry.0 += tag.unread_messages;
                entry.1 += tag.total_messages;
            }
        }
        let tags = tag_totals
            .into_iter()
            .map(|(name, (unread_messages, total_messages))| TagSummary {
                name,
                unread_messages,
                total_messages,
            })
            .collect();

        Ok(SidebarResponse {
            smart_mailboxes: sidebar_smart_mailboxes,
            tags,
            sources: sidebar_sources,
        })
    }

    // -- Store delegates (runtime data) --

    /// List all mailboxes for an account.
    pub fn list_mailboxes(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MailboxSummary>, ServiceError> {
        self.mailbox_reader
            .list_mailboxes(account_id)
            .map_err(Into::into)
    }

    /// Update server-side mailbox metadata and refresh the local mailbox projection.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    /// @spec docs/L1-jmap#methods-used
    pub async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        role: Option<&str>,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let expected_state = self
            .sync_state
            .get_cursor(account_id, SyncObject::Mailbox)?;
        let clear_role_from = match role {
            Some(role) => self
                .mailbox_reader
                .list_mailboxes(account_id)?
                .into_iter()
                .find(|mailbox| mailbox.id != *mailbox_id && mailbox.role.as_deref() == Some(role))
                .map(|mailbox| mailbox.id),
            None => None,
        };
        gateway
            .set_mailbox_role(
                account_id,
                mailbox_id,
                expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                role,
                clear_role_from.as_ref(),
            )
            .await?;
        self.sync_account(account_id, SyncTrigger::Manual, gateway, None)
            .await
    }

    /// List messages, optionally filtered by mailbox.
    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.message_lister
            .list_messages(account_id, mailbox_id)
            .map_err(Into::into)
    }

    /// Paginated message list with seek-based cursors.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    pub fn list_message_page(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        self.message_lister
            .list_message_page(
                account_id,
                mailbox_id,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )
            .map_err(Into::into)
    }

    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    pub fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        self.conversation_reader
            .list_conversations(
                account_id,
                mailbox_id,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )
            .map_err(Into::into)
    }

    /// Fetch a single conversation with all its messages, or 404.
    pub fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, ServiceError> {
        self.conversation_reader
            .get_conversation(conversation_id)?
            .not_found("conversation", conversation_id.as_str())
    }

    /// Fetch all messages in a thread, or 404.
    pub fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<ThreadView, ServiceError> {
        self.message_detail_reader
            .get_thread(account_id, thread_id)?
            .not_found("thread", thread_id.as_str())
    }

    /// Fetch message detail, lazily fetching body from the gateway if needed.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: Option<&dyn MailGateway>,
    ) -> Result<CommandResult, ServiceError> {
        let detail = self
            .message_detail_reader
            .get_message_detail(account_id, message_id)?
            .not_found("message", message_id.as_str())?;

        let body_loaded = detail.body_html.is_some() || detail.body_text.is_some();
        let attachments_loaded = !detail.summary.has_attachment || !detail.attachments.is_empty();
        if body_loaded && attachments_loaded {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        }

        let Some(gateway) = gateway else {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        };

        let fetched = gateway.fetch_message_body(account_id, message_id).await?;
        self.sync_writer
            .apply_message_body(account_id, message_id, &fetched)
            .map_err(Into::into)
    }

    /// Re-score dirty cache candidates after local utility signals change.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn process_cache_rescore_batch(
        &self,
        account_id: &AccountId,
        batch_size: usize,
    ) -> Result<CacheRescoreBatchOutcome, ServiceError> {
        let mut outcome = CacheRescoreBatchOutcome::default();
        if batch_size == 0 {
            return Ok(outcome);
        }

        let candidates = self
            .cache_store
            .list_cache_rescore_candidates(account_id, batch_size)?;
        outcome.scanned = candidates.len();
        if candidates.is_empty() {
            debug!(
                account_id = %account_id,
                "cache rescore worker found no dirty candidates"
            );
            return Ok(outcome);
        }

        let account = self.config.get_source(account_id)?;
        if account.is_none()
            && candidates
                .iter()
                .any(|candidate| candidate.layer == CacheLayer::Body)
        {
            return Err(StoreError::NotFound(format!("source:{}", account_id.as_str())).into());
        }
        let updates = candidates
            .iter()
            .map(|candidate| {
                let (fetch_unit, value_bytes, fetch_bytes) = match (&account, candidate.layer) {
                    (Some(account), CacheLayer::Body) => {
                        let fetch_unit = body_fetch_unit(account);
                        (
                            fetch_unit,
                            estimated_body_bytes_from_metadata(
                                candidate.message_size,
                                candidate.has_attachment,
                            ),
                            body_fetch_bytes_from_metadata(
                                account,
                                candidate.message_size,
                                candidate.has_attachment,
                            ),
                        )
                    }
                    _ => (
                        candidate.fetch_unit,
                        candidate.value_bytes,
                        candidate.fetch_bytes,
                    ),
                };
                let signals =
                    rescore_candidate_signals(candidate, fetch_unit, value_bytes, fetch_bytes);
                let score = score_cache_candidate(&signals);
                trace!(
                    account_id = %account_id,
                    message_id = candidate.message_id.as_str(),
                    layer = candidate.layer.as_str(),
                    fetch_unit = fetch_unit.as_str(),
                    value_bytes,
                    fetch_bytes,
                    old_priority = candidate.priority,
                    new_priority = score.priority,
                    utility = score.utility,
                    size_cost = score.size_cost,
                    signal_reason = candidate.signal_reason.as_str(),
                    rescore_priority = candidate.rescore_priority,
                    direct_user_boost = candidate.direct_user_boost,
                    search_result_rank = candidate.search.as_ref().map(|search| search.result_rank),
                    "cache candidate re-scored"
                );
                CachePriorityUpdate {
                    account_id: candidate.account_id.clone(),
                    message_id: candidate.message_id.clone(),
                    layer: candidate.layer,
                    object_id: candidate.object_id.clone(),
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    priority: score.priority,
                    reason: candidate.signal_reason.clone(),
                }
            })
            .collect::<Vec<_>>();
        self.cache_store.update_cache_priorities(&updates)?;
        outcome.updated = updates.len();
        debug!(
            account_id = %account_id,
            scanned = outcome.scanned,
            updated = outcome.updated,
            "cache rescore worker batch completed"
        );
        Ok(outcome)
    }

    /// Queue stale cache objects for re-scoring so time-sensitive utility, such
    /// as recency, converges even without new sync or search signals.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn queue_stale_cache_rescore_batch(
        &self,
        account_id: &AccountId,
        stale_after: Duration,
        batch_size: usize,
    ) -> Result<usize, ServiceError> {
        if batch_size == 0 {
            return Ok(0);
        }
        let stale_seconds = i64::try_from(stale_after.as_secs()).unwrap_or(i64::MAX);
        let stale_before = (time::OffsetDateTime::now_utc()
            - time::Duration::seconds(stale_seconds))
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| StoreError::Failure(err.to_string()))?;
        let queued = self.cache_store.queue_stale_cache_rescore_candidates(
            account_id,
            stale_before.as_str(),
            batch_size,
        )?;
        if queued > 0 {
            debug!(
                account_id = %account_id,
                stale_after_seconds = stale_after.as_secs(),
                stale_before = stale_before.as_str(),
                queued,
                "stale cache candidates queued for re-score"
            );
        }
        Ok(queued)
    }

    /// Fetch one bounded batch of wanted message-body cache candidates.
    ///
    /// The first worker slice has no eviction path, so it admits only bodies
    /// that fit under the current effective background target.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub async fn process_body_cache_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        lease: CacheFetchLease,
    ) -> Result<CacheWorkerBatchOutcome, ServiceError> {
        let mut outcome = CacheWorkerBatchOutcome::default();
        if !lease.has_fetch_budget() {
            return Ok(outcome);
        }

        let settings = self.config.get_app_settings()?;
        if !settings.cache_policy.cache_bodies {
            debug!(
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                "cache worker skipped because body caching is disabled"
            );
            return Ok(outcome);
        }

        let mut used_bytes = self.cache_store.cache_used_bytes()?;
        let scan_limit = lease
            .request_limit
            .saturating_mul(4)
            .max(lease.request_limit);
        let initial_budget = settings
            .cache_policy
            .clone()
            .budget(used_bytes, lease.interactive_pressure);
        let candidates = self.cache_store.list_cache_fetch_candidates(
            account_id,
            CacheLayer::Body,
            scan_limit,
        )?;
        debug!(
            account_id = %account_id,
            layer = CacheLayer::Body.as_str(),
            request_limit = lease.request_limit,
            byte_limit = lease.byte_limit,
            scan_limit,
            candidate_count = candidates.len(),
            used_bytes,
            soft_cap_bytes = initial_budget.soft_cap_bytes,
            effective_target_bytes = initial_budget.effective_target_bytes(),
            hard_cap_bytes = initial_budget.hard_cap_bytes,
            interactive_pressure = initial_budget.interactive_pressure,
            "cache worker body batch planned"
        );
        if candidates.is_empty() {
            debug!(
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                "cache worker found no wanted body candidates"
            );
        }
        let mut remaining_lease_bytes = lease.byte_limit;
        for candidate in candidates {
            if outcome.attempted >= lease.request_limit {
                break;
            }
            outcome.scanned += 1;
            if candidate.fetch_bytes > remaining_lease_bytes {
                outcome.skipped += 1;
                debug!(
                    account_id = %account_id,
                    message_id = candidate.message_id.as_str(),
                    layer = candidate.layer.as_str(),
                    fetch_unit = candidate.fetch_unit.as_str(),
                    fetch_bytes = candidate.fetch_bytes,
                    remaining_lease_bytes,
                    "cache candidate deferred by fetch byte lease"
                );
                continue;
            }
            let budget = settings
                .cache_policy
                .clone()
                .budget(used_bytes, lease.interactive_pressure);
            let admission =
                decide_cache_admission(candidate.fetch_bytes, candidate.priority, None, &budget);
            debug!(
                account_id = %account_id,
                message_id = candidate.message_id.as_str(),
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                priority = candidate.priority,
                admission = ?admission,
                used_bytes = budget.used_bytes,
                effective_target_bytes = budget.effective_target_bytes(),
                hard_cap_bytes = budget.hard_cap_bytes,
                "cache candidate admission evaluated"
            );
            if admission != CacheAdmission::AdmitWithinTarget {
                outcome.skipped += 1;
                continue;
            }

            let message_id = MessageId::from(candidate.message_id.as_str());
            self.cache_store.mark_cache_object_state(
                account_id,
                &message_id,
                candidate.layer,
                candidate.object_id.as_deref(),
                CacheObjectState::Fetching,
                None,
            )?;
            outcome.attempted += 1;
            outcome.attempted_bytes = outcome
                .attempted_bytes
                .saturating_add(candidate.fetch_bytes);
            remaining_lease_bytes = remaining_lease_bytes.saturating_sub(candidate.fetch_bytes);
            debug!(
                account_id = %account_id,
                message_id = %message_id,
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                priority = candidate.priority,
                "cache candidate fetch started"
            );

            let fetched = match gateway.fetch_message_body(account_id, &message_id).await {
                Ok(fetched) => fetched,
                Err(error) => {
                    let service_error = ServiceError::from(error);
                    let error_code = service_error.code().to_string();
                    debug!(
                        account_id = %account_id,
                        message_id = %message_id,
                        layer = candidate.layer.as_str(),
                        fetch_unit = candidate.fetch_unit.as_str(),
                        error_code = error_code.as_str(),
                        "cache candidate fetch failed"
                    );
                    self.cache_store.mark_cache_object_state(
                        account_id,
                        &message_id,
                        candidate.layer,
                        candidate.object_id.as_deref(),
                        CacheObjectState::Failed,
                        Some(error_code.as_str()),
                    )?;
                    outcome.failed += 1;
                    continue;
                }
            };

            let result = self
                .sync_writer
                .apply_message_body(account_id, &message_id, &fetched)?;
            self.cache_store.mark_cache_object_state(
                account_id,
                &message_id,
                candidate.layer,
                candidate.object_id.as_deref(),
                CacheObjectState::Cached,
                None,
            )?;
            used_bytes = used_bytes.saturating_add(candidate.fetch_bytes);
            outcome.cached += 1;
            outcome.cached_bytes = outcome.cached_bytes.saturating_add(candidate.fetch_bytes);
            outcome.events.extend(result.events);
            debug!(
                account_id = %account_id,
                message_id = %message_id,
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                used_bytes,
                "cache candidate stored"
            );
        }

        Ok(outcome)
    }

    /// Download a blob for a specific account via the registered gateway.
    pub async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &crate::BlobId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<u8>, ServiceError> {
        gateway
            .download_blob(account_id, blob_id)
            .await
            .map_err(Into::into)
    }

    fn upsert_body_cache_candidates(
        &self,
        account_id: &AccountId,
        account: &AccountSettings,
        policy: &CachePolicy,
        messages: &[MessageRecord],
    ) -> Result<(), ServiceError> {
        if !policy.cache_bodies || messages.is_empty() {
            debug!(
                account_id = %account_id,
                message_count = messages.len(),
                cache_bodies = policy.cache_bodies,
                "cache candidate generation skipped"
            );
            return Ok(());
        }

        let inbox_mailbox_ids = self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .filter(|mailbox| mailbox.role.as_deref() == Some("inbox"))
            .map(|mailbox| mailbox.id)
            .collect::<HashSet<_>>();
        let fetch_unit = body_fetch_unit(account);
        let candidates = messages
            .iter()
            .filter(|message| message.body_html.is_none() && message.body_text.is_none())
            .map(|message| {
                let value_bytes = estimated_body_bytes(message);
                let fetch_bytes = body_fetch_bytes(account, message);
                let signals = CacheCandidateSignals {
                    message: CacheMessageSignals {
                        age_days: message_age_days(&message.received_at),
                        in_inbox: message
                            .mailbox_ids
                            .iter()
                            .any(|mailbox_id| inbox_mailbox_ids.contains(mailbox_id)),
                        unread: !message.keywords.iter().any(|keyword| keyword == "$seen"),
                        flagged: message.keywords.iter().any(|keyword| keyword == "$flagged"),
                        thread_activity: 0.0,
                        sender_affinity: 0.0,
                        local_behavior: 0.0,
                        search: None,
                    },
                    layer: CacheLayer::Body,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    inline_attachment: false,
                    opened_attachment: false,
                    direct_user_boost: 0.0,
                    pinned: false,
                };
                let score = score_cache_candidate(&signals);
                trace!(
                    account_id = %account_id,
                    message_id = %message.id,
                    layer = CacheLayer::Body.as_str(),
                    fetch_unit = fetch_unit.as_str(),
                    value_bytes,
                    fetch_bytes,
                    utility = score.utility,
                    size_cost = score.size_cost,
                    priority = score.priority,
                    age_days = signals.message.age_days,
                    in_inbox = signals.message.in_inbox,
                    unread = signals.message.unread,
                    flagged = signals.message.flagged,
                    "cache body candidate scored"
                );
                CacheCandidate {
                    account_id: account_id.to_string(),
                    message_id: message.id.to_string(),
                    layer: CacheLayer::Body,
                    object_id: None,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    priority: score.priority,
                    reason: match fetch_unit {
                        CacheFetchUnit::BodyOnly => "body".to_string(),
                        CacheFetchUnit::RawMessage => "body-via-raw-message".to_string(),
                        CacheFetchUnit::AttachmentBlob => "body".to_string(),
                    },
                }
            })
            .collect::<Vec<_>>();
        let total_fetch_bytes = candidates
            .iter()
            .map(|candidate| candidate.fetch_bytes)
            .sum::<u64>();
        let total_value_bytes = candidates
            .iter()
            .map(|candidate| candidate.value_bytes)
            .sum::<u64>();
        debug!(
            account_id = %account_id,
            driver = ?account.driver,
            fetch_unit = fetch_unit.as_str(),
            synced_message_count = messages.len(),
            candidate_count = candidates.len(),
            total_value_bytes,
            total_fetch_bytes,
            "cache body candidates scored"
        );
        self.cache_store.upsert_cache_candidates(&candidates)?;
        debug!(
            account_id = %account_id,
            candidate_count = candidates.len(),
            "cache body candidates upserted"
        );
        Ok(())
    }

    /// Run a full sync cycle: load cursors, fetch delta, apply batch, emit events.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let cursors = self.sync_state.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors, progress.clone()).await?;
        if let Some(progress) = progress {
            progress.report(crate::SyncProgress {
                sync_id: String::new(),
                trigger: trigger.clone(),
                started_at: String::new(),
                stage: crate::SyncProgressStage::Storing,
                detail: "Applying synced changes".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: Some(batch.messages.len()),
                total_count: None,
            });
        }
        let mut events = self.sync_writer.apply_sync_batch(account_id, &batch)?;
        if let Some(account) = self.config.get_source(account_id)? {
            let settings = self.config.get_app_settings()?;
            self.upsert_body_cache_candidates(
                account_id,
                &account,
                &settings.cache_policy,
                &batch.messages,
            )?;
        }
        let action_events = self
            .apply_automation_rules(account_id, &batch.messages, gateway)
            .await?;
        let action_count = action_events.len();
        events.extend(action_events);
        let sync_event = self.events.append_event(
            account_id,
            EVENT_TOPIC_SYNC_COMPLETED,
            None,
            None,
            json!({
                "mailboxCount": batch.mailboxes.len(),
                "messageCount": batch.messages.len(),
                "deletedMessageCount": batch.deleted_message_ids.len(),
                "automationEventCount": action_count,
                "trigger": trigger.as_str(),
            }),
        )?;
        events.push(sync_event);
        Ok(events)
    }

    async fn apply_automation_rules(
        &self,
        account_id: &AccountId,
        messages: &[MessageRecord],
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        if self.config.get_source(account_id)?.is_none() {
            return Ok(Vec::new());
        }
        let settings = self.config.get_app_settings()?;
        if settings.automation_rules.is_empty() || messages.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::new();
        let message_ids = messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        for rule in settings.automation_rules.iter().filter(|rule| {
            rule.enabled
                && rule
                    .triggers
                    .iter()
                    .any(|trigger| trigger == &AutomationTrigger::MessageArrived)
        }) {
            for action in &rule.actions {
                let query_rule = automation_query_rule(account_id, rule, action, &message_ids);
                let page = self.smart_mailboxes.query_message_page_by_rule(
                    &query_rule,
                    messages.len(),
                    None,
                    MessageSortField::Date,
                    SortDirection::Asc,
                )?;
                for message in page.items {
                    let result = self
                        .apply_automation_action(account_id, &message, action, gateway)
                        .await?;
                    events.extend(result.events);
                }
            }
        }
        Ok(events)
    }

    async fn apply_automation_action(
        &self,
        account_id: &AccountId,
        message: &MessageSummary,
        action: &AutomationAction,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        match action {
            AutomationAction::ApplyTag { tag } => {
                if message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec![tag.clone()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::RemoveTag { tag } => {
                if !message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec![tag.clone()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MarkRead => {
                if message.is_read {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$seen".to_string()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MarkUnread => {
                if !message.is_read {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$seen".to_string()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::Flag => {
                if message.is_flagged {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$flagged".to_string()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::Unflag => {
                if !message.is_flagged {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$flagged".to_string()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MoveToMailbox { mailbox_id } => {
                if message.mailbox_ids.len() == 1
                    && message
                        .mailbox_ids
                        .iter()
                        .any(|candidate| candidate == mailbox_id)
                {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.replace_mailboxes(
                    account_id,
                    &message.id,
                    &ReplaceMailboxesCommand {
                        mailbox_ids: vec![mailbox_id.clone()],
                    },
                    gateway,
                )
                .await
            }
        }
    }

    /// Process one durable low-priority automation backfill batch for an account.
    ///
    /// The current rules are fingerprinted before work starts. A completed job
    /// suppresses repeated scans for the same rules, while changed rules create
    /// a new pending job.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub async fn process_automation_backfill_job_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
    ) -> Result<AutomationBackfillBatchOutcome, ServiceError> {
        if batch_size == 0 {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }
        let Some(source) = self.config.get_source(account_id)? else {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        };
        if !source.enabled {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        };

        let job = self
            .automation_backfills
            .ensure_automation_backfill_job(account_id, &rule_fingerprint)?;
        if job.status != AutomationBackfillJobStatus::Pending {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }

        match self
            .backfill_automation_rules_batch_with_settings(
                account_id, gateway, batch_size, &settings,
            )
            .await
        {
            Ok((events, has_more)) => {
                if !has_more {
                    self.automation_backfills
                        .complete_automation_backfill_job(account_id, &rule_fingerprint)?;
                }
                Ok(AutomationBackfillBatchOutcome {
                    ran: true,
                    events,
                    has_more,
                })
            }
            Err(error) => {
                self.automation_backfills
                    .record_automation_backfill_failure(
                        account_id,
                        &rule_fingerprint,
                        &error.to_string(),
                    )?;
                Err(error)
            }
        }
    }

    /// Apply one bounded batch of global automation rules to existing local mail.
    ///
    /// This is intended for low-priority background backfill. It queries the
    /// local projection first, then applies actions through JMAP so the server
    /// remains authoritative.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub async fn backfill_automation_rules_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
    ) -> Result<(Vec<DomainEvent>, bool), ServiceError> {
        if self.config.get_source(account_id)?.is_none() {
            return Ok((Vec::new(), false));
        }
        let settings = self.config.get_app_settings()?;
        self.backfill_automation_rules_batch_with_settings(
            account_id, gateway, batch_size, &settings,
        )
        .await
    }

    async fn backfill_automation_rules_batch_with_settings(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
        settings: &AppSettings,
    ) -> Result<(Vec<DomainEvent>, bool), ServiceError> {
        if settings.automation_rules.is_empty() || batch_size == 0 {
            return Ok((Vec::new(), false));
        }

        let mut events = Vec::new();
        let mut has_more = false;
        let mut remaining = batch_size;

        for rule in settings
            .automation_rules
            .iter()
            .filter(|rule| rule.enabled && rule.backfill)
        {
            if remaining == 0 {
                has_more = true;
                break;
            }
            for action in &rule.actions {
                if remaining == 0 {
                    has_more = true;
                    break;
                }
                let query_rule = automation_query_rule(account_id, rule, action, &[]);
                let page = self.smart_mailboxes.query_message_page_by_rule(
                    &query_rule,
                    remaining,
                    None,
                    MessageSortField::Date,
                    SortDirection::Asc,
                )?;
                if page.items.len() == remaining {
                    has_more = true;
                }

                for message in page.items {
                    let result = self
                        .apply_automation_action(account_id, &message, action, gateway)
                        .await?;
                    if !result.events.is_empty() {
                        remaining -= 1;
                    }
                    events.extend(result.events);
                    if remaining == 0 {
                        has_more = true;
                        break;
                    }
                }
            }
        }

        Ok((events, has_more))
    }

    /// Append a `sync.failed` event to the event log.
    ///
    /// @spec docs/L1-sync#error-handling
    pub fn record_sync_failure(
        &self,
        account_id: &AccountId,
        code: &str,
        message: &str,
        trigger: SyncTrigger,
        stage: &str,
    ) -> Result<DomainEvent, ServiceError> {
        self.events
            .append_event(
                account_id,
                EVENT_TOPIC_SYNC_FAILED,
                None,
                None,
                json!({
                    "code": code,
                    "message": message,
                    "trigger": trigger.as_str(),
                    "stage": stage,
                }),
            )
            .map_err(Into::into)
    }

    /// Apply a message mutation: send to gateway with optimistic concurrency,
    /// then persist locally with the returned cursor.
    ///
    /// @spec docs/L1-sync#conflict-model
    async fn apply_message_mutation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        mutation: MessageMutation<'_>,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self
            .sync_state
            .get_cursor(account_id, SyncObject::Message)?;
        let outcome = match mutation {
            MessageMutation::SetKeywords(command) => {
                gateway
                    .set_keywords(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                        command,
                    )
                    .await?
            }
            MessageMutation::ReplaceMailboxes(command) => {
                gateway
                    .replace_mailboxes(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                        &command.mailbox_ids,
                    )
                    .await?
            }
            MessageMutation::Destroy => {
                gateway
                    .destroy_message(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                    )
                    .await?
            }
        };

        match mutation {
            MessageMutation::SetKeywords(command) => self.message_commands.set_keywords(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
                command,
            ),
            MessageMutation::ReplaceMailboxes(command) => self.message_commands.replace_mailboxes(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
                command,
            ),
            MessageMutation::Destroy => self.message_commands.destroy_message(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
            ),
        }
        .map_err(Into::into)
    }

    /// Add/remove JMAP keywords on a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::SetKeywords(command),
            gateway,
        )
        .await
    }

    /// Atomically replace all mailbox memberships for a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::ReplaceMailboxes(command),
            gateway,
        )
        .await
    }

    /// Add a message to a mailbox (idempotent: no-op if already present).
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn add_to_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &AddToMailboxCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        let mut mailbox_ids = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?;
        if !mailbox_ids
            .iter()
            .any(|mailbox_id| mailbox_id == &command.mailbox_id)
        {
            mailbox_ids.push(command.mailbox_id.clone());
        }
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
            gateway,
        )
        .await
    }

    /// Remove a message from a single mailbox.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn remove_from_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &RemoveFromMailboxCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        let mailbox_ids = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?
            .into_iter()
            .filter(|mailbox_id| mailbox_id != &command.mailbox_id)
            .collect();
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
            gateway,
        )
        .await
    }

    /// Permanently delete a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(account_id, message_id, MessageMutation::Destroy, gateway)
            .await
    }

    /// Query the event log with optional filters.
    ///
    /// @spec docs/L1-api#sse-event-stream
    pub fn list_events(
        &self,
        filter: &crate::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.events.list_events(filter).map_err(Into::into)
    }

    /// Fetch the primary sender identity from the gateway.
    ///
    /// @spec docs/L1-jmap#methods-used
    pub async fn fetch_identity(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Identity, ServiceError> {
        gateway.fetch_identity(account_id).await.map_err(Into::into)
    }

    /// Fetch reply/forward metadata for composing a response.
    pub async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<crate::ReplyContext, ServiceError> {
        gateway
            .fetch_reply_context(account_id, message_id)
            .await
            .map_err(Into::into)
    }

    /// Send an email via the gateway.
    ///
    /// @spec docs/L1-jmap#methods-used
    pub async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
        gateway: &dyn MailGateway,
    ) -> Result<(), ServiceError> {
        gateway
            .send_message(account_id, request)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::{
        AutomationBackfillStore, CacheFetchCandidate, CachePriorityUpdate, CacheRescoreCandidate,
        CacheSignalUpdate, CachedSenderAddress, ConfigError, ConfigSnapshot, ConversationReadStore,
        DomainEvent, EventFilter, EventStore, FetchedBody, GatewayError, ImapMailboxSyncState,
        ImapMessageLocation, ImapMessageLocationStore, ImapSyncStateStore, MailboxReadStore,
        MessageCommandStore, MessageDetail, MessageDetailStore, MessageListStore,
        MessageMailboxStore, MutationOutcome, PushTransport, Recipient, SenderAddressCacheStore,
        SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
        SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
        SmartMailboxStore, SmartMailboxValue, SourceDataStore, SourceProjectionStore, StoreError,
        SyncBatch, SyncCursor, SyncStateStore, SyncWriteStore,
    };

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

        fn get_smart_mailbox(
            &self,
            id: &SmartMailboxId,
        ) -> Result<Option<SmartMailbox>, ConfigError> {
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
        applied_bodies: Mutex<Vec<(MessageId, Option<String>, Option<String>)>>,
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
        fn list_mailboxes(
            &self,
            _account_id: &AccountId,
        ) -> Result<Vec<MailboxSummary>, StoreError> {
            self.list_mailboxes_error
                .as_ref()
                .map_or(Ok(Vec::new()), |error| {
                    Err(StoreError::Failure(error.clone()))
                })
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
            Ok(Vec::new())
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
            _batch: &SyncBatch,
        ) -> Result<Vec<DomainEvent>, StoreError> {
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
            if let Some(job) = jobs.iter().find(|job| {
                &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint
            }) {
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
            if let Some(job) = jobs.iter_mut().find(|job| {
                &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint
            }) {
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
            if let Some(job) = jobs.iter_mut().find(|job| {
                &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint
            }) {
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
                .find(|job| {
                    &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint
                })
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
        let gateway = MutationGateway::with_fetch_body_result(Err(GatewayError::Network(
            "offline".to_string(),
        )));

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
            vec![(
                MessageId::from("message-1"),
                CacheObjectState::Fetching,
                None
            )]
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

    #[tokio::test]
    async fn genuine_state_mismatch_is_not_retried() {
        let account = AccountId::from("primary");
        let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
        let config = Arc::new(TestConfig::default());
        let service = MailService::new(store.clone(), config);
        let gateway = MutationGateway::with_revision(2);

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
            "message-1"
        );
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
}
