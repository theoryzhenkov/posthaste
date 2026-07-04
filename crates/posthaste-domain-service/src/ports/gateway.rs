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
/// `async` (D63/M23b): the service-side implementation
/// ([`ServiceSyncSink`](../service/sync_ops/struct.ServiceSyncSink.html))
/// applies the chunk to the store — the heaviest store write on the sync path
/// — via `tokio::task::spawn_blocking` (the store's `SyncWriteStore` port
/// stays a plain sync trait; see its doc comment), so `emit` must be awaitable
/// rather than a blocking synchronous call made from a tokio worker. Transport
/// streaming loops that page results into `emit` (e.g. the JMAP page fetcher
/// in `posthaste-engine`) call it with `.await` per page/chunk — they are
/// already `async fn`s driving other awaited requests, so this is a
/// same-shape addition, not a new async boundary.
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
#[async_trait]
pub trait SyncChunkSink: Send {
    async fn emit(&mut self, batch: SyncBatch) -> Result<(), GatewayError>;
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
    async fn sync_streamed(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
        sink: &mut dyn SyncChunkSink,
    ) -> Result<SyncOutcome, GatewayError> {
        let batch = self.sync(account_id, cursors, progress).await?;
        sink.emit(batch).await?;
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

    /// Send an email via `EmailSubmission/set` (JMAP) or SMTP submission.
    ///
    /// `idempotency_key` is the outbox operation id — stable across retries. The
    /// gateway derives its send identity from it (JMAP: a deterministic
    /// `EmailSubmission` create-id + `ifInState`; SMTP/JMAP: a stable
    /// `Message-ID`) so a re-forward of a send that already committed is
    /// deduplicated rather than duplicated (D84/D85). On a timeout or a
    /// transport loss where the submission may already have committed, the
    /// implementation returns [`GatewayError::DispatchUncertain`] so the outbox
    /// parks the send instead of blind-resending it (D86).
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
        idempotency_key: &str,
    ) -> Result<(), GatewayError>;

    /// Persist a draft to the provider's Drafts mailbox, returning the new
    /// message id. When `replace` is set, the prior draft message is removed
    /// (drafts are immutable in JMAP, so an update is create-new + destroy-old).
    ///
    /// `idempotent_redelivery` narrows the replace-destroy `notFound ⇒ Ok` mask
    /// (DS3/D133), mirroring [`delete_draft`]: `true` (a redelivered save whose
    /// prior-draft destroy already ran) treats an already-gone replace target as
    /// success; `false` (a first delivery) surfaces a failed replace-destroy as a
    /// retryable failure rather than a clean save that silently left the old
    /// draft behind (the twin).
    ///
    /// Default transport behaviour rejects draft writes; JMAP and IMAP override.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
        _replace: Option<&MessageId>,
        _idempotent_redelivery: bool,
    ) -> Result<MessageId, GatewayError> {
        Err(GatewayError::Rejected(
            "draft writes are not supported by this transport".to_string(),
        ))
    }

    /// Delete a draft message from the provider's Drafts mailbox.
    ///
    /// Default transport behaviour rejects draft deletion; JMAP and IMAP override.
    ///
    /// `idempotent_redelivery` narrows the `notFound ⇒ Ok` mask (D133): `true`
    /// (a send-consume redelivery) treats an already-gone draft as success;
    /// `false` (a user-initiated discard) surfaces `notFound` as a retryable
    /// failure so the client reverts the optimistic fold and shows the error.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _idempotent_redelivery: bool,
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
