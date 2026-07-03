//! The authority server's read channel (D29 split of `authority_server.rs`):
//! account/settings/mailbox/tag reads, draft + resource reads, the mail-query
//! engine entry points, and the fold-state read the down-channel authors
//! complete base assertions from. Verbatim moves from `authority_server.rs`.
use super::*;

impl AuthorityServer {
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

    /// Read channel: the cheap `event_log` seq bounds for the fact-carrying tap's
    /// head/truncation queries (RFC-L2-scripting S2). `None` when the log is
    /// empty.
    pub(crate) fn event_log_bounds(
        &self,
    ) -> Result<Option<EventLogBounds>, RuntimeError> {
        Ok(self.service.event_log_bounds()?)
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
    /// the authority's ([replication authority-server-link L3](../replication/authority-server-link/L3.md)). A near node
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

    /// The message's current canonical fold state (keywords + mailbox
    /// membership) read from the authoritative store, or `None` if it is gone.
    ///
    /// The far node authors **complete** base assertions: individual command
    /// events do not all carry the full post-state (a mailbox move event omits
    /// keywords), but `MessageReplica`'s base is a whole-message replace, so the
    /// down-channel reads the current summary to assert the complete state
    /// ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)).
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
}
