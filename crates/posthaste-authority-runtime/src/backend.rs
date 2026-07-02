//! The backend far node: the single owner of message-command backend access.
//!
//! This is the **far node** of the runtime↔backend coherent link
//! ([replication backend-link L1 §2-§3](../replication/backend-link/L1.md)). It owns the `MailService` +
//! store and is the one place message-state commands cross from the runtime into
//! the backend: each applies the command to the service, publishes the resulting
//! authoritative domain events, and nudges the provider outbox to flush.
//!
//! Today it is reached **in-process** (co-located), through
//! [`LocalBackend`](crate::local_backend::LocalBackend): the runtime
//! calls it directly, zero serialization, identical to the pre-link behavior
//! (assertion `colocated-unchanged`). Extracting it as a named type is the W1
//! seam — the runtime no longer reaches the backend by scattered direct
//! `service`/`store` calls on the mutation path; it goes through this far node.
//!
//! Reads stay on the runtime's direct store access for now; W2 moves the
//! runtime's served views onto a near-node base cache fed by this node's
//! down-channel, at which point reads stop crossing the link too.
//!
//! @spec docs/replication/backend-link/L1#3-the-backendapi-contract

use std::collections::BTreeMap;
use std::sync::Arc;

use posthaste_domain_service::{
    now_iso8601, AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress,
    CommandAck, ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, Identity,
    MailService, MailStore, MailboxId, MailboxSummary, MessageDetail, MessageId, MessageSummary,
    Operation, OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext,
    RevLogSnapshot, SendMessageRequest, ServiceErrorKind, SetKeywordsCommand, SharedGateway,
    SmartMailbox, SmartMailboxId, SmartMailboxSummary, StoreError, SyncMode, SyncTrigger,
    TagSummary, EVENT_TOPIC_REV_LOG_APPENDED,
};
use posthaste_link_core::{MessageChangeDiff, MessageFoldState};
use posthaste_observability::{events, ph_warn};
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, PatchAppSettingsMutation,
    PatchSmartMailboxMutation, RevCursorArgs, RevStepInput, RuntimeAccountList,
    RuntimeError, RuntimeErrorCode, RuntimeResourceBytes,
};
use tokio::sync::{broadcast, mpsc};

use crate::account_reads::AccountReadService;
use crate::live_accounts::LiveAccountRuntimeProvider;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::runtime_registry::{ForwardAcceptance, RuntimeRegistry};
use posthaste_link_contract::{
    message_mutation::MessageMutation, DownFrame, RuntimeId, WireMutationId,
    WireSettlementOutcome,
};
use posthaste_runtime_contract::mutation_args::keyword_toggle;

/// The backend far node ([replication backend-link L1 §3](../replication/backend-link/L1.md)): owns the
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
    runtimes: RuntimeRegistry,
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
            runtimes: RuntimeRegistry::new(),
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
    pub(crate) fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
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
    pub(crate) fn list_sender_addresses(&self) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
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
                    .download_blob(
                        &account_id,
                        &message_id,
                        &attachment.blob_id,
                        gateway.as_ref(),
                    )
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
    /// the authority's ([replication backend-link L3](../replication/backend-link/L3.md)). A near node
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
    pub(crate) fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.mail_queries.message_detail(account_id, message_id)
    }

    /// Read channel: an overlay-folded conversation for the `conversation` view.
    pub(crate) fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.mail_queries.conversation(conversation_id)
    }

    /// Read channel: the count of live (running) accounts (the supervisor's),
    /// for the runtime status.
    pub(crate) fn account_count(&self) -> Option<usize> {
        self.live_accounts.account_count()
    }

    /// Read channel: the per-account undo/redo `rev_log` + cursor (Phase 2
    /// server-authoritative history). Serves the `RevLog` synced view.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    pub(crate) fn rev_log_snapshot(
        &self,
        account_id: &AccountId,
    ) -> Result<RevLogSnapshot, RuntimeError> {
        self.store
            .rev_log_snapshot(account_id)
            .map_err(|error| RuntimeError::internal(error.to_string(), None))
    }

    /// Phase 2: on a confirmed forward action whose `context` carries a
    /// `revStep`, append the reversible-op step to `rev_log` + emit
    /// [`EVENT_TOPIC_REV_LOG_APPENDED`] so the `RevLog` synced view re-serves
    /// the log + cursor. Best-effort — a store failure is logged (the mutation
    /// already applied; the client can retry the append by re-sending the step).
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn append_rev_log_step_if_present(
        &self,
        account_id: &str,
        message_id: &str,
        context: &Option<serde_json::Value>,
    ) {
        let Some(rev_step) = context.as_ref().and_then(|c| c.get("revStep")) else {
            return;
        };
        let Ok(rev_step) = serde_json::from_value::<RevStepInput>(rev_step.clone()) else {
            ph_warn!(
                events::REV_LOG_APPEND_FAILED,
                account_id = %account_id,
                "rev_log step payload in mutation context was invalid; skipping append"
            );
            return;
        };
        let account = AccountId(account_id.to_string());
        let created_at = now_iso8601().unwrap_or_default();
        match self.store.append_rev_log_step(
            &account,
            &rev_step.step_id,
            message_id,
            account_id,
            &rev_step.diff,
            &created_at,
        ) {
            Ok(_) => {
                let _ = self.event_sender.send(DomainEvent {
                    seq: 0,
                    account_id: account.clone(),
                    topic: EVENT_TOPIC_REV_LOG_APPENDED.to_string(),
                    occurred_at: created_at,
                    mailbox_id: None,
                    message_id: Some(MessageId(message_id.to_string())),
                    payload: serde_json::json!({ "stepId": rev_step.step_id }),
                });
            }
            Err(error) => ph_warn!(
                events::REV_LOG_APPEND_FAILED,
                account_id = %account_id,
                step_id = %rev_step.step_id,
                error = %error,
                "rev_log append failed; the mutation applied but is not yet undoable"
            ),
        }
    }

    /// Phase 2: apply a `revCursor` control mutation — validate the referenced
    /// steps exist in `rev_log`, then apply the idempotent cursor assignment +
    /// emit `rev_log.appended` so the `RevLog` synced view re-serves the
    /// cursor. Re-delivery is a no-op (the assignment is idempotent).
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn apply_rev_cursor(&self, request: &MutationRequest) -> Result<CommandAck, RuntimeError> {
        let args: RevCursorArgs = serde_json::from_value(request.args.clone())
            .map_err(|e| RuntimeError::invalid_mutation(format!("invalid revCursor args: {e}")))?;
        let account = AccountId(args.account_id.clone());
        // Validate: cursor_step_id (if Some) + redo_tail steps must exist.
        let snapshot = self
            .store
            .rev_log_snapshot(&account)
            .map_err(|e| RuntimeError::internal(e.to_string(), None))?;
        if let Some(cursor) = &args.cursor_step_id {
            if !snapshot.steps.iter().any(|s| &s.step_id == cursor) {
                return Err(RuntimeError::invalid_mutation(format!(
                    "revCursor cursor_step_id {cursor} is not in the rev_log"
                )));
            }
        }
        for step in &args.redo_tail {
            if !snapshot.steps.iter().any(|s| &s.step_id == step) {
                return Err(RuntimeError::invalid_mutation(format!(
                    "revCursor redo_tail step {step} is not in the rev_log"
                )));
            }
        }
        // Apply the idempotent cursor assignment.
        self.store
            .set_rev_cursor(&account, args.cursor_step_id.as_deref(), &args.redo_tail)
            .map_err(|e| RuntimeError::internal(e.to_string(), None))?;
        // Emit the recompute trigger (same topic as append).
        let _ = self.event_sender.send(DomainEvent {
            seq: 0,
            account_id: account,
            topic: EVENT_TOPIC_REV_LOG_APPENDED.to_string(),
            occurred_at: now_iso8601().unwrap_or_default(),
            mailbox_id: None,
            message_id: None,
            payload: serde_json::json!({
                "cursorStepId": args.cursor_step_id,
                "redoTail": args.redo_tail,
            }),
        });
        Ok(CommandAck { events: Vec::new() })
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
    /// ([`LocalBackend`](crate::local_backend::LocalBackend)).
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_sender.subscribe()
    }

    /// Get the originating runtime's settlement receiver (for `subscribe_for` to
    /// merge with this `Base` broadcast). Reconnect-safe: a reconnecting runtime
    /// gets a fresh receiver (S4).
    pub(crate) fn subscribe_settlement(
        &self,
        runtime_id: &RuntimeId,
    ) -> mpsc::UnboundedReceiver<DownFrame> {
        self.runtimes.subscribe_settlement(runtime_id)
    }

    /// The message's current canonical fold state (keywords + mailbox
    /// membership) read from the authoritative store, or `None` if it is gone.
    ///
    /// The far node authors **complete** base assertions: individual command
    /// events do not all carry the full post-state (a mailbox move event omits
    /// keywords), but `MessageReplica`'s base is a whole-message replace, so the
    /// down-channel reads the current summary to assert the complete state
    /// ([replication backend-link L1 §3](../replication/backend-link/L1.md)).
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
        let result = self
            .service
            .destroy_message(&account_id, &message_id)
            .await?;
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
    pub(crate) fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
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

    pub(crate) async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
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

    /// `message.snooze`: move to the Snoozed mailbox (the one with the `snooze`
    /// role) + record the return time. Reuses `move_message_to_role` for the
    /// provider move; the move's `replace_mailboxes_tx` invariant clears any
    /// prior snooze row, then we insert the new one. Rejects if no mailbox has
    /// the `snooze` role (the user must designate one via the role switch).
    /// @spec docs/eph/DESIGN-L2-snooze
    pub(crate) async fn snooze_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        until: i64,
    ) -> Result<CommandAck, RuntimeError> {
        let ack = self
            .move_message_to_role(account_id.clone(), message_id.clone(), "snooze".to_string())
            .await?;
        self.store
            .insert_snooze(&account_id, &message_id, until)
            .map_err(store_error_to_runtime_error)?;
        Ok(ack)
    }

    /// `message.unsnooze`: move a snoozed message back to the Inbox. The store
    /// invariant (`replace_mailboxes_tx` clears the snooze row when a message
    /// leaves the Snoozed mailbox) handles the return-time cleanup.
    /// @spec docs/eph/DESIGN-L2-snooze
    pub(crate) async fn unsnooze_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        self.move_message_to_role(account_id, message_id, "inbox".to_string())
            .await
    }

    /// Up-channel for a (possibly remote) runtime: dedup by
    /// `(RuntimeId, ClientMutationId)`, then apply the named mutation and return
    /// a receipt carrying the backend's `RuntimeMutationId` for the confirmation
    /// join. A retried mutation resolves to its stored record, never a second
    /// application (`per-runtime-idempotency`). The co-located runtime passes a
    /// real minted id (it is runtime #1 of X, X=1 in-process — no single-runtime
    /// special case); a remote runtime's id is derived from its credential.
    ///
    /// @spec docs/replication/backend-link/L1#3-the-backendapi-contract
    pub(crate) async fn forward_mutation_for(
        &self,
        runtime_id: &RuntimeId,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        match self
            .runtimes
            .accept(runtime_id, &mutation.client_mutation_id, &mutation.name)
        {
            ForwardAcceptance::Existing(receipt) => Ok(receipt),
            ForwardAcceptance::New { runtime_mutation_id } => {
                let ack = match self.apply_named_message_mutation(&mutation).await {
                    Ok(ack) => ack,
                    Err(error) => {
                        // The mutation did not apply (atomic): drop the reserved
                        // entry so a retry re-accepts as New, and surface the error
                        // on the up-channel. No Settlement — the near node learns of
                        // the failure from the up-channel error and cannot match a
                        // Settlement it never received a receipt for.
                        self.runtimes.reject(runtime_id, &mutation.client_mutation_id);
                        return Err(error);
                    }
                };
                let output = serde_json::to_value(&ack).map_err(|error| {
                    RuntimeError::internal(
                        format!("failed to serialize mutation output: {error}"),
                        None,
                    )
                })?;
                self.runtimes
                    .settle_output(runtime_id, &mutation.client_mutation_id, output.clone());
                // Route the per-mutation confirmation onto the originating
                // runtime's down-stream only (`settlement-routed-to-origin-runtime`):
                // never broadcast — a Settlement names one runtime's mutation.
                self.runtimes.emit_settlement(
                    runtime_id,
                    DownFrame::Settlement {
                        mutation_id: WireMutationId(runtime_mutation_id.as_str().to_string()),
                        outcome: WireSettlementOutcome::Confirmed,
                    },
                );
                Ok(MutationReceipt {
                    runtime_mutation_id: Some(runtime_mutation_id),
                    client_mutation_id: mutation.client_mutation_id,
                    name: mutation.name,
                    state: MutationSettlementState::Accepted,
                    error: None,
                    output,
                })
            }
        }
    }

    /// Apply one named message mutation — the backend's up-channel handler. This
    /// is the dispatch from a transport-neutral named mutation
    /// (`message.setKeywords` / `message.moveToRole` / …) to the typed command,
    /// moved here from the runtime: the backend "accepts named mutations"
    /// ([replication backend-link L1 §3](../replication/backend-link/L1.md)). The runtime keeps the
    /// session/undo/scope concerns around this call; this node only applies the
    /// effect and returns the resulting events.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    pub(crate) async fn apply_named_message_mutation(
        &self,
        request: &MutationRequest,
    ) -> Result<CommandAck, RuntimeError> {
        // Phase 2: `revCursor` is a control mutation (not a message mutation) —
        // route it to the cursor-arbitration path before the message parse.
        if request.name == "revCursor" {
            return self.apply_rev_cursor(request);
        }
        let mutation = MessageMutation::from_request(request)?;
        let account = AccountId(mutation.account_id().to_string());
        let message = MessageId(mutation.message_id().to_string());
        let ack = match mutation {
            MessageMutation::SetKeywords(args) => {
                self.set_keywords(account.clone(), message.clone(), args.command)
                    .await
            }
            MessageMutation::SetReadState(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    keyword_toggle("$seen", args.read),
                )
                .await
            }
            MessageMutation::SetFlaggedState(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    keyword_toggle("$flagged", args.flagged),
                )
                .await
            }
            MessageMutation::SetUserTags(args) => {
                self.set_keywords(
                    account.clone(),
                    message.clone(),
                    SetKeywordsCommand {
                        add: args.add,
                        remove: args.remove,
                    },
                )
                .await
            }
            MessageMutation::MoveToMailbox(args) => {
                self.replace_mailboxes(
                    account.clone(),
                    message.clone(),
                    ReplaceMailboxesCommand {
                        mailbox_ids: vec![MailboxId(args.mailbox_id)],
                    },
                )
                .await
            }
            MessageMutation::ReplaceMailboxes(args) => {
                self.replace_mailboxes(
                    account.clone(),
                    message.clone(),
                    ReplaceMailboxesCommand {
                        mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                    },
                )
                .await
            }
            MessageMutation::MoveToRole(args) => {
                self.move_message_to_role(account.clone(), message.clone(), args.role)
                    .await
            }
            MessageMutation::Snooze(args) => {
                self.snooze_message(account.clone(), message.clone(), args.until)
                    .await
            }
            MessageMutation::Unsnooze(_) => {
                self.unsnooze_message(account.clone(), message.clone()).await
            }
            MessageMutation::Destroy(_) => {
                self.destroy(account.clone(), message.clone()).await
            }
            // `message.applyDiff` is the undo/redo vehicle — see `apply_diff`.
            MessageMutation::ApplyDiff(args) => {
                self.apply_diff(account.clone(), message.clone(), args.diff).await
            }
        }?;
        // Phase 2: append the reversible-op step on a confirmed forward action
        // whose context carries a `revStep`, + emit the recompute trigger so the
        // `RevLog` synced view re-serves the log + cursor.
        self.append_rev_log_step_if_present(account.as_str(), message.as_str(), &request.context);
        Ok(ack)
    }

    /// `message.applyDiff`: apply the invertible diff as the equivalent keyword
    /// add/remove plus a mailbox add/remove. Keywords are a delta
    /// (`SetKeywordsCommand`); mailboxes are computed against the current
    /// membership and applied as one replace. The far-node mirror of the
    /// near-node `ApplyDiff` assertion fold.
    async fn apply_diff(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        diff: MessageChangeDiff,
    ) -> Result<CommandAck, RuntimeError> {
        let mut events = Vec::new();
        if !diff.keywords.added.is_empty() || !diff.keywords.removed.is_empty() {
            let ack = self
                .set_keywords(
                    account_id.clone(),
                    message_id.clone(),
                    SetKeywordsCommand {
                        add: diff.keywords.added,
                        remove: diff.keywords.removed,
                    },
                )
                .await?;
            events.extend(ack.events);
        }
        if !diff.mailboxes.added.is_empty() || !diff.mailboxes.removed.is_empty() {
            let mut mailbox_ids: Vec<MailboxId> = self
                .current_summary(&account_id, &message_id)
                .await?
                .map(|summary| summary.mailbox_ids)
                .unwrap_or_default();
            for added in &diff.mailboxes.added {
                let id = MailboxId(added.clone());
                if !mailbox_ids.contains(&id) {
                    mailbox_ids.push(id);
                }
            }
            mailbox_ids.retain(|id| !diff.mailboxes.removed.iter().any(|r| r == id.as_str()));
            let ack = self
                .replace_mailboxes(account_id, message_id, ReplaceMailboxesCommand { mailbox_ids })
                .await?;
            events.extend(ack.events);
        }
        Ok(CommandAck { events })
    }
}

/// Map a store-layer failure to an internal runtime error — the shape the
/// runtime handle used before these reads moved to the far node.
fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
}
