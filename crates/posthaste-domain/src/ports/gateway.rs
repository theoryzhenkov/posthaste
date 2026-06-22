use super::*;
use async_trait::async_trait;

/// Gateway to a remote JMAP server.
///
/// Abstracts JMAP protocol operations behind a domain-level interface.
/// Implementations: `LiveJmapGateway` for real JMAP, `MockGateway` for tests.
///
/// @spec docs/L1-jmap#methods-used
/// Receives sync chunks from [`MailGateway::sync_streamed`] to apply and publish
/// as they arrive. The service supplies the implementation; the gateway only
/// emits.
///
/// @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
pub trait SyncChunkSink: Send {
    fn emit(&mut self, batch: SyncBatch) -> Result<(), GatewayError>;
}

#[async_trait]
pub trait MailGateway: Send + Sync {
    /// Perform a delta or full sync for all object types using stored cursors.
    ///
    /// @spec docs/L1-sync#sync-loop
    async fn sync(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError>;

    /// Stream a sync as chunks, handing each to `sink` to apply + publish as it
    /// is fetched, so mail surfaces progressively instead of arriving in one
    /// batch. Returns a [`SyncOutcome`] whose reconciliation set (when present)
    /// the service prunes against in a final pass.
    ///
    /// The default emits a single chunk — the full [`sync`](Self::sync) batch,
    /// which carries its own `replace_all` pruning and cursors — so it is
    /// self-reconciling and behaves exactly like the batch path. Folder-centric
    /// (IMAP) and page-centric (JMAP) transports override this to emit chunks.
    ///
    /// @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
    async fn sync_streamed(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
        sink: &mut dyn SyncChunkSink,
    ) -> Result<SyncOutcome, GatewayError> {
        let batch = self.sync(account_id, cursors, progress).await?;
        sink.emit(batch)?;
        Ok(SyncOutcome::single_batch())
    }

    /// Lazily fetch body content for a single message.
    ///
    /// @spec docs/L1-sync#sync-loop
    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError>;

    /// Download an attachment or inline blob by its JMAP `blobId`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError>;

    /// Extract a blob's bytes from already-cached raw RFC822 message bytes.
    ///
    /// Lets callers serve attachments from a previously fetched message instead
    /// of re-downloading it. Returns `Ok(None)` when this transport cannot
    /// resolve the blob from raw bytes (the default), in which case callers fall
    /// back to [`download_blob`]. Adapters whose blob ids index into the raw
    /// MIME (such as IMAP) override this.
    fn extract_cached_blob(
        &self,
        _blob_id: &BlobId,
        _raw_mime: &[u8],
    ) -> Result<Option<Vec<u8>>, GatewayError> {
        Ok(None)
    }

    /// Update JMAP keywords on a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Atomically replace all mailbox memberships for a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError>;

    /// Permanently delete a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Update a mailbox role via `Mailbox/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Fetch the primary sender identity via `Identity/get`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, GatewayError>;

    /// Fetch reply/forward metadata for composing a response.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError>;

    /// Send an email via `EmailSubmission/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError>;

    /// Persist a draft to the provider's Drafts mailbox, returning the new
    /// message id. When `replace` is set, the prior draft message is removed
    /// (drafts are immutable in JMAP, so an update is create-new + destroy-old).
    ///
    /// Default transport behaviour rejects draft writes; JMAP and IMAP override.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
        _replace: Option<&MessageId>,
    ) -> Result<MessageId, GatewayError> {
        Err(GatewayError::Rejected(
            "draft writes are not supported by this transport".to_string(),
        ))
    }

    /// Delete a draft message from the provider's Drafts mailbox.
    ///
    /// Default transport behaviour rejects draft deletion; JMAP and IMAP override.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected(
            "draft deletion is not supported by this transport".to_string(),
        ))
    }

    /// Return available push transports ordered by preference (WS first, then SSE).
    ///
    /// @spec docs/L2-transport#new-abstractions
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>>;
}
