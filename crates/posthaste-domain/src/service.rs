use std::sync::Arc;

use posthaste_observability::{events, ph_warn};
use serde_json::json;

use crate::{
    AccountId, AccountSettings, AppSettings, AutomationBackfillStore, CacheStore, CommandResult,
    ConfigDiff, ConfigRepository, ConversationCursor, ConversationId, ConversationPage,
    ConversationReadStore, ConversationSortField, ConversationView, EventStore, Identity,
    MailGateway, MailStore, MailboxId, MailboxReadStore, MailboxSummary, MessageCommandStore,
    MessageCursor, MessageDetailStore, MessageId, MessageListStore, MessageMailboxStore,
    MessagePage, MessageSortField, MessageSummary, SendMessageRequest, ServiceError,
    SharedConfigRepository, SmartMailbox, SmartMailboxId, SmartMailboxRule, SmartMailboxStore,
    SmartMailboxSummary, SortDirection, SourceDataStore, SourceProjectionStore, SyncMode,
    SyncObject, SyncStateStore, SyncTrigger, SyncWriteStore, TagReadStore, TagSummary, ThreadId,
    ThreadView, EVENT_TOPIC_SYNC_COMPLETED, EVENT_TOPIC_SYNC_FAILED,
};
use crate::{DomainEvent, ServiceResultExt};

mod automation;
mod cache;
mod mutation;
#[cfg(test)]
mod tests;

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

    /// List user-facing tags for one account.
    pub fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, ServiceError> {
        self.tag_reader.list_tags(account_id).map_err(Into::into)
    }

    /// List user-facing tags merged across the provided accounts.
    pub fn list_merged_tags(
        &self,
        account_ids: &[AccountId],
    ) -> Result<Vec<TagSummary>, ServiceError> {
        let mut tag_totals = std::collections::BTreeMap::<String, (i64, i64)>::new();
        for account_id in account_ids {
            for tag in self.tag_reader.list_tags(account_id)? {
                let entry = tag_totals.entry(tag.name).or_insert((0, 0));
                entry.0 += tag.unread_messages;
                entry.1 += tag.total_messages;
            }
        }
        Ok(tag_totals
            .into_iter()
            .map(|(name, (unread_messages, total_messages))| TagSummary {
                name,
                unread_messages,
                total_messages,
            })
            .collect())
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
        self.sync_account_with_mode(
            account_id,
            trigger,
            SyncMode::Incremental,
            gateway,
            progress,
        )
        .await
    }

    /// Run a sync cycle with an explicit user-requested mode.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        mode: SyncMode,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut cursors = self.sync_state.get_sync_cursors(account_id)?;
        if mode.requires_full_message_metadata() {
            cursors.retain(|cursor| cursor.object_type != SyncObject::Message);
        }
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
        let mut post_commit_errors = Vec::new();
        if let Some(account) = self.config.get_source(account_id)? {
            let settings = self.config.get_app_settings()?;
            if let Err(error) = self.upsert_body_cache_candidates(
                account_id,
                &account,
                &settings.cache_policy,
                &batch.messages,
            ) {
                ph_warn!(
                    events::DOMAIN_CACHE_CANDIDATE_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync body cache candidate update failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
            }
        }
        let action_events = match self
            .apply_automation_rules(account_id, &batch.messages, gateway)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                ph_warn!(
                    events::DOMAIN_AUTOMATION_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync automation failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
                Vec::new()
            }
        };
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
                "deletedImapLocationCount": batch.deleted_imap_message_locations.len(),
                "deletedMessageCount": batch.deleted_message_ids.len(),
                "automationEventCount": action_count,
                "trigger": trigger.as_str(),
                "mode": mode.as_str(),
                "resources": [
                    { "kind": "sync", "operation": "completed", "accountId": account_id.as_str(), "mode": mode.as_str() },
                    { "kind": "mailbox", "operation": "refreshed", "accountId": account_id.as_str() },
                    { "kind": "message", "operation": "refreshed", "accountId": account_id.as_str() },
                ],
                "postCommitErrors": post_commit_errors,
            }),
        )?;
        events.push(sync_event);
        Ok(events)
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
                    "resources": [
                        { "kind": "sync", "operation": "failed", "accountId": account_id.as_str() },
                        { "kind": "accountRuntime", "operation": "updated", "accountId": account_id.as_str() },
                    ],
                }),
            )
            .map_err(Into::into)
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
