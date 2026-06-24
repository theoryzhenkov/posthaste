//! The backend far node: the single owner of message-command backend access.
//!
//! This is the **far node** of the runtime↔backend coherent link
//! ([replication L4 §2-§3](../replication/L4.md)). It owns the `MailService` +
//! store and is the one place message-state commands cross from the runtime into
//! the backend: each applies the command to the service, publishes the resulting
//! authoritative domain events, and nudges the provider outbox to flush.
//!
//! Today it is reached **in-process** (co-located), through
//! [`InProcessTransport`](crate::transport::InProcessTransport): the runtime
//! calls it directly, zero serialization, identical to the pre-link behavior
//! (assertion `colocated-unchanged`). Extracting it as a named type is the W1
//! seam — the runtime no longer reaches the backend by scattered direct
//! `service`/`store` calls on the mutation path; it goes through this far node.
//!
//! Reads stay on the runtime's direct store access for now; W2 moves the
//! runtime's served views onto a near-node base cache fed by this node's
//! down-channel, at which point reads stop crossing the link too.
//!
//! @spec docs/replication/L4#3-the-link-contract-backendlink

use std::collections::BTreeMap;
use std::sync::Arc;

use posthaste_domain::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailService,
    MailStore, MailboxId, MailboxSummary, MessageDetail, MessageId, MessageSummary, Operation,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext, ServiceErrorKind,
    SetKeywordsCommand, SharedGateway, SmartMailbox, SmartMailboxId, SmartMailboxSummary,
    OperationId, SendMessageRequest, StoreError, SyncMode, SyncTrigger, TagSummary,
};
use posthaste_link_core::MessageFoldState;
use posthaste_observability::{events, ph_warn};
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationRequest, PatchAccountMutation,
    PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAccountList, RuntimeError,
    RuntimeErrorCode, RuntimeResourceBytes,
};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::account_reads::AccountReadService;
use crate::live_accounts::LiveAccountRuntimeProvider;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;

/// Build a single-keyword add/remove command from a desired presence. Shared by
/// the backend's read-state/flagged-state application and the runtime's history
/// capture for the same mutations.
pub(crate) fn keyword_toggle(keyword: &str, present: bool) -> SetKeywordsCommand {
    if present {
        SetKeywordsCommand {
            add: vec![keyword.to_string()],
            remove: Vec::new(),
        }
    } else {
        SetKeywordsCommand {
            add: Vec::new(),
            remove: vec![keyword.to_string()],
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetKeywordsMutationArgs {
    pub source_id: String,
    pub message_id: String,
    pub command: SetKeywordsCommand,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetReadStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetFlaggedStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub flagged: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageSetUserTagsArgs {
    pub source_id: String,
    pub message_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageMoveToMailboxArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageMoveToRoleArgs {
    pub source_id: String,
    pub message_id: String,
    pub role: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageReplaceMailboxesArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_ids: Vec<String>,
}

/// A message mutation that targets one message by id (archive/trash/destroy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MessageTargetArgs {
    pub source_id: String,
    pub message_id: String,
}

/// The backend far node ([replication L4 §3](../replication/L4.md)): owns the
/// service + store + the live-account supervisor + the event publisher, and
/// applies message-state commands to them.
pub(crate) struct Backend {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    mail_queries: Arc<MailQueryService>,
    account_reads: Arc<AccountReadService>,
    account_mutations: Option<Arc<AccountMutationService>>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    event_sender: broadcast::Sender<DomainEvent>,
}

impl Backend {
    pub(crate) fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        mail_queries: Arc<MailQueryService>,
        account_reads: Arc<AccountReadService>,
        account_mutations: Option<Arc<AccountMutationService>>,
        live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            service,
            store,
            mail_queries,
            account_reads,
            account_mutations,
            live_accounts,
            event_sender,
        }
    }

    /// The account/config mutation service, or the not-ready error when this
    /// backend was built without one (some migration/test compositions).
    fn account_mutations(&self) -> Result<&AccountMutationService, RuntimeError> {
        self.account_mutations.as_deref().ok_or_else(|| {
            RuntimeError::runtime_not_ready("account mutation runtime is not available")
        })
    }

    /// Resolve a best-effort gateway for the account, swallowing the error: the
    /// draft/resource reads serve cached data offline when no live gateway is
    /// available.
    async fn optional_gateway(&self, account_id: &AccountId) -> Option<SharedGateway> {
        self.live_accounts.gateway(account_id).await.ok()
    }

    /// Read channel: the account list.
    pub(crate) async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        Ok(self.account_reads.list_accounts().await?)
    }

    /// Read channel: one account's overview (`None` when absent).
    pub(crate) async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        Ok(self.account_reads.get_account(account_id).await?)
    }

    /// Read channel: the application settings.
    pub(crate) fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        Ok(self.account_reads.app_settings()?)
    }

    /// Read channel: resolve an account scope to concrete ids.
    pub(crate) fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        Ok(self.account_reads.resolve_account_scope(scope)?)
    }

    /// Read channel: mailboxes per account for a scope.
    pub(crate) fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.account_reads.list_mailboxes(scope).map_err(|error| {
            if error.kind() == ServiceErrorKind::NotFound {
                RuntimeError::with_details(
                    RuntimeErrorCode::NotFound,
                    "account not found",
                    serde_json::json!({}),
                )
            } else {
                error.into()
            }
        })
    }

    /// Read channel: the smart-mailbox summaries.
    pub(crate) fn list_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        Ok(self.account_reads.list_smart_mailboxes()?)
    }

    /// Read channel: one smart mailbox.
    pub(crate) fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        Ok(self.account_reads.get_smart_mailbox(&smart_mailbox_id)?)
    }

    /// Read channel: the tag summaries for a scope.
    pub(crate) fn list_tags(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError> {
        Ok(self.account_reads.list_tags(scope)?)
    }

    /// Read channel: the account's sender identity (resolving a live gateway).
    pub(crate) async fn get_identity(
        &self,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError> {
        let gateway = self.live_accounts.gateway(&account_id).await?;
        Ok(self
            .service
            .fetch_identity(&account_id, gateway.as_ref())
            .await?)
    }

    /// Read channel: reply/forward metadata for one message (resolving a live
    /// gateway).
    pub(crate) async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        let gateway = self.live_accounts.gateway(&account_id).await?;
        Ok(self
            .service
            .fetch_reply_context(&account_id, &message_id, gateway.as_ref())
            .await?)
    }

    /// Read channel: the cached sender addresses.
    pub(crate) fn list_sender_addresses(
        &self,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.store
            .list_sender_address_cache()
            .map_err(store_error_to_runtime_error)
    }

    /// Read channel: an account's pending outbox operations.
    pub(crate) fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        Ok(self.service.list_pending_operations(&account_id)?)
    }

    /// Read channel: replay the authoritative event log for a filter.
    pub(crate) fn replay_events(
        &self,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        Ok(self.service.list_events(&filter)?)
    }

    /// Read channel: compose-ready content for resuming a draft. Lazily fetches
    /// the body when a gateway is available, publishing the resulting events on
    /// the down-channel.
    pub(crate) async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .service
            .get_draft_content(&account_id, &message_id, gateway.as_deref())
            .await?;
        self.publish_events(&result.events);
        Ok(result.content)
    }

    /// Read channel: the raw bytes of a message resource — an attachment blob, or
    /// the HTML/text body. Body resources return raw bytes (the serve layer
    /// applies the per-kind transform); attachments may download from the
    /// gateway. Lazily fetches detail when a gateway is available, publishing the
    /// resulting events on the down-channel.
    pub(crate) async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .service
            .get_message_detail(&account_id, &message_id, gateway.as_deref())
            .await?;
        self.publish_events(&result.events);
        let detail = result
            .detail
            .ok_or_else(|| RuntimeError::not_found("message detail not available"))?;
        match kind {
            MessageResourceKind::Attachment(attachment_id) => {
                let attachment = detail
                    .attachments
                    .into_iter()
                    .find(|attachment| attachment.id == attachment_id)
                    .ok_or_else(|| RuntimeError::not_found("attachment not found"))?;
                let gateway = gateway.ok_or_else(|| {
                    RuntimeError::retryable(
                        RuntimeErrorCode::ProviderUnavailable,
                        format!("gateway unavailable for account {account_id}"),
                    )
                })?;
                let bytes = self
                    .service
                    .download_blob(&account_id, &message_id, &attachment.blob_id, gateway.as_ref())
                    .await?;
                Ok(RuntimeResourceBytes {
                    bytes,
                    content_type: attachment.mime_type,
                    filename: attachment.filename,
                    inline_attachments: Vec::new(),
                })
            }
            // Body resources return RAW bytes; the server serve layer applies the
            // per-kind transform (HTML sanitization + inline-URL rewrite) before
            // responding — the runtime never sanitizes. Body HTML carries its
            // inline attachments so the server can rewrite `cid:` URLs.
            MessageResourceKind::BodyHtml => Ok(RuntimeResourceBytes {
                bytes: detail.body_html.unwrap_or_default().into_bytes(),
                content_type: "text/html; charset=utf-8".to_string(),
                filename: None,
                inline_attachments: detail.attachments,
            }),
            MessageResourceKind::BodyText => Ok(RuntimeResourceBytes {
                bytes: detail.body_text.unwrap_or_default().into_bytes(),
                content_type: "text/plain; charset=utf-8".to_string(),
                filename: None,
                inline_attachments: Vec::new(),
            }),
        }
    }

    /// Read channel: compute a page of a mail-list query — the query engine is
    /// the authority's ([replication L4 W4](../replication/L4.md)). A near node
    /// reads through here.
    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.mail_queries.query_mail_page(request).await
    }

    /// Read channel: the message's current canonical summary (the point read
    /// behind undo-history).
    pub(crate) async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        let result = self
            .service
            .get_message_detail(account_id, message_id, None)
            .await?;
        Ok(result.detail.map(|detail| detail.summary))
    }

    /// Read channel: a message's detail (header + attachments, body-free) for the
    /// `messageDetail` view.
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.mail_queries.message_detail(account_id, message_id).await
    }

    /// Read channel: an overlay-folded conversation for the `conversation` view.
    pub(crate) fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.mail_queries.conversation(conversation_id)
    }

    /// Publish authoritative domain events on the down-channel broadcast. In the
    /// co-located deployment this is the same event bus the runtime's views and
    /// the SSE event stream already consume.
    pub(crate) fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    /// A receiver on the authoritative domain-event broadcast — the raw signal
    /// the link's down-channel is built from
    /// ([`InProcessTransport::subscribe`](crate::transport::InProcessTransport)).
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_sender.subscribe()
    }

    /// The message's current canonical fold state (keywords + mailbox
    /// membership) read from the authoritative store, or `None` if it is gone.
    ///
    /// The far node authors **complete** base assertions: individual command
    /// events do not all carry the full post-state (a mailbox move event omits
    /// keywords), but `MessageReplica`'s base is a whole-message replace, so the
    /// down-channel reads the current summary to assert the complete state
    /// ([replication L4 §3](../replication/L4.md)).
    pub(crate) fn current_fold_state(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageFoldState>, RuntimeError> {
        let detail = self.service.get_message_header(account_id, message_id)?;
        Ok(detail.map(|detail| MessageFoldState {
            keywords: detail.summary.keywords,
            mailbox_ids: detail
                .summary
                .mailbox_ids
                .iter()
                .map(|mailbox_id| mailbox_id.as_str().to_string())
                .collect(),
        }))
    }

    /// Nudge the account to sync so just-enqueued outbox operations flush
    /// promptly. Best-effort: if the account is offline the op stays queued and
    /// flushes on the next connectivity window.
    pub(crate) async fn trigger_outbox_flush(&self, account_id: &AccountId) {
        if let Err(error) = self
            .live_accounts
            .trigger_account_sync(account_id, SyncTrigger::Manual)
            .await
        {
            ph_warn!(
                events::OUTBOX_FOLLOWUP_SYNC_TRIGGER_FAILED,
                source_id = %account_id,
                error = %error,
                "outbox operation enqueued but follow-up sync trigger failed"
            );
        }
    }

    pub(crate) async fn set_keywords(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .set_keywords(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn add_to_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .add_to_mailbox(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn remove_from_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .remove_from_mailbox(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn replace_mailboxes(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self
            .service
            .replace_mailboxes(&account_id, &message_id, &command)
            .await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn destroy(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        let result = self.service.destroy_message(&account_id, &message_id).await?;
        self.publish_events(&result.events);
        self.trigger_outbox_flush(&account_id).await;
        Ok(result)
    }

    pub(crate) async fn set_mailbox_role(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        let gateway = self.live_accounts.gateway(&account_id).await?;
        let events = self
            .service
            .set_mailbox_role(&account_id, &mailbox_id, role.as_deref(), gateway.as_ref())
            .await?;
        self.publish_events(&events);
        Ok(self.service.list_mailboxes(&account_id)?)
    }

    /// Write: queue a local-first send and nudge a flush. No live gateway is
    /// required to accept it; it flushes on the next connectivity window.
    pub(crate) async fn send_message(
        &self,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        let sender = request.from.clone();
        self.service.enqueue_send(&account_id, request)?;
        if let Some(sender) = &sender {
            if let Err(error) = self.store.remember_sender_address(&account_id, sender) {
                ph_warn!(
                    events::SEND_SENDER_CACHE_UPDATE_FAILED,
                    source_id = %account_id,
                    sender = %sender.email,
                    error = %error,
                    "send accepted but sender address cache update failed"
                );
            }
        }
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    /// Write: save (create or update) a draft and nudge a flush.
    pub(crate) async fn save_draft(
        &self,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        let operation = self.service.save_draft(&account_id, draft_id, request)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    /// Write: delete a draft and nudge a flush.
    pub(crate) async fn delete_draft(
        &self,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        let operation = self.service.delete_draft(&account_id, draft_id)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    /// Write: discard a pending outbox operation.
    pub(crate) fn discard_operation(
        &self,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.service.discard_operation(&operation_id)?;
        Ok(())
    }

    /// Write: re-arm a failed outbox operation to pending and nudge a flush.
    pub(crate) async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.service.retry_operation(&operation_id)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    /// Write: drive an explicit account sync, returning the number of changes.
    pub(crate) async fn sync_account(
        &self,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        Ok(self
            .live_accounts
            .sync_account_with_mode(&account_id, mode)
            .await?)
    }

    // ===== Account + config mutations (account_mutations authority) =====

    pub(crate) fn patch_app_settings(
        &self,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.account_mutations()?.patch_app_settings(mutation)
    }

    pub(crate) fn preview_automation_rule(
        &self,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        self.account_mutations()?.preview_automation_rule(mutation)
    }

    pub(crate) fn create_smart_mailbox(
        &self,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.account_mutations()?.create_smart_mailbox(mutation)
    }

    pub(crate) fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.account_mutations()?
            .patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    pub(crate) fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .delete_smart_mailbox(smart_mailbox_id)
    }

    pub(crate) fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.account_mutations()?.reset_default_smart_mailboxes()
    }

    pub(crate) async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.account_mutations()?.create_account(mutation).await
    }

    pub(crate) async fn patch_account(
        &self,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.account_mutations()?
            .patch_account(account_id, mutation)
            .await
    }

    pub(crate) async fn delete_account(
        &self,
        account_id: AccountId,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?.delete_account(account_id).await
    }

    pub(crate) async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.account_mutations()?.verify_account(account_id).await
    }

    pub(crate) async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .set_account_enabled(account_id, enabled)
            .await
    }

    pub(crate) async fn reload_config(&self) -> Result<(), RuntimeError> {
        self.account_mutations()?.reload_config().await
    }

    /// Resolve the account's mailbox for `role` and replace the message's
    /// mailbox membership with it. Role resolution is backend-owned so the
    /// runtime forwards role intent without looking up role mailboxes.
    ///
    /// @spec docs/state/mail/L1#message-change-assertions
    pub(crate) async fn move_message_to_role(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        role: String,
    ) -> Result<CommandAck, RuntimeError> {
        let mailbox = self
            .service
            .list_mailboxes(&account_id)?
            .into_iter()
            .find(|mailbox| mailbox.role.as_deref() == Some(role.as_str()))
            .ok_or_else(|| {
                RuntimeError::invalid_mutation(format!("account has no mailbox with role '{role}'"))
            })?;
        self.replace_mailboxes(
            account_id,
            message_id,
            ReplaceMailboxesCommand {
                mailbox_ids: vec![mailbox.id],
            },
        )
        .await
    }

    /// Apply one named message mutation — the backend's up-channel handler. This
    /// is the dispatch from a transport-neutral named mutation
    /// (`message.setKeywords` / `message.archive` / …) to the typed command,
    /// moved here from the runtime: the backend "accepts named mutations"
    /// ([replication L4 §3](../replication/L4.md)). The runtime keeps the
    /// session/undo/scope concerns around this call; this node only applies the
    /// effect and returns the resulting events.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    pub(crate) async fn apply_named_message_mutation(
        &self,
        request: &MutationRequest,
    ) -> Result<CommandAck, RuntimeError> {
        match request.name.as_str() {
            "message.setKeywords" => {
                let args: MessageSetKeywordsMutationArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    args.command,
                )
                .await
            }
            "message.setReadState" => {
                let args: MessageSetReadStateArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    keyword_toggle("$seen", args.read),
                )
                .await
            }
            "message.setFlaggedState" => {
                let args: MessageSetFlaggedStateArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    keyword_toggle("$flagged", args.flagged),
                )
                .await
            }
            "message.setUserTags" => {
                let args: MessageSetUserTagsArgs = parse_args(request)?;
                self.set_keywords(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    SetKeywordsCommand {
                        add: args.add,
                        remove: args.remove,
                    },
                )
                .await
            }
            "message.moveToMailbox" => {
                let args: MessageMoveToMailboxArgs = parse_args(request)?;
                self.replace_mailboxes(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    ReplaceMailboxesCommand {
                        mailbox_ids: vec![MailboxId(args.mailbox_id)],
                    },
                )
                .await
            }
            "message.replaceMailboxes" => {
                let args: MessageReplaceMailboxesArgs = parse_args(request)?;
                self.replace_mailboxes(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    ReplaceMailboxesCommand {
                        mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                    },
                )
                .await
            }
            "message.moveToRole" => {
                let args: MessageMoveToRoleArgs = parse_args(request)?;
                self.move_message_to_role(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    args.role,
                )
                .await
            }
            "message.archive" | "message.trash" | "message.restoreToInbox" => {
                let args: MessageTargetArgs = parse_args(request)?;
                let role = match request.name.as_str() {
                    "message.archive" => "archive",
                    "message.trash" => "trash",
                    _ => "inbox",
                };
                self.move_message_to_role(
                    AccountId(args.source_id),
                    MessageId(args.message_id),
                    role.to_string(),
                )
                .await
            }
            "message.destroy" => {
                let args: MessageTargetArgs = parse_args(request)?;
                self.destroy(AccountId(args.source_id), MessageId(args.message_id))
                    .await
            }
            _ => Err(RuntimeError::invalid_mutation(format!(
                "unknown runtime mutation '{}'",
                request.name
            ))),
        }
    }
}

/// Map a store-layer failure to an internal runtime error — the shape the
/// runtime handle used before these reads moved to the far node.
fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
}

pub(crate) fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(request.args.clone()).map_err(|error| {
        RuntimeError::with_details(
            posthaste_runtime_contract::RuntimeErrorCode::InvalidMutation,
            format!("invalid args for mutation '{}'", request.name),
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}
