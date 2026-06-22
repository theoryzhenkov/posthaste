use async_trait::async_trait;
use posthaste_domain::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MessageId,
    MutationOutcome, PushTransport, ReplyContext, SendMessageRequest, SetKeywordsCommand,
    SyncBatch, SyncChunkSink, SyncCursor, SyncOutcome,
};

use super::LiveJmapGateway;

/// @spec docs/L1-jmap#method-calls
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L2-transport#gateway-unchanged
#[async_trait]
impl MailGateway for LiveJmapGateway {
    /// Perform a full sync cycle: mailbox state then email state.
    ///
    /// Falls back from delta to full sync on `cannotCalculateChanges`.
    ///
    /// @spec docs/L1-sync#sync-loop
    /// @spec docs/L1-sync#state-management
    async fn sync(
        &self,
        _account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<posthaste_domain::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        crate::live_sync::sync_account(&self.client, cursors, progress).await
    }

    /// Stream the sync as chunks: mailbox state then email metadata pages, so
    /// mail surfaces progressively. A full snapshot returns a reconciliation
    /// set; a delta self-reconciles.
    ///
    /// @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
    async fn sync_streamed(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<posthaste_domain::SyncProgressReporter>,
        sink: &mut dyn SyncChunkSink,
    ) -> Result<SyncOutcome, GatewayError> {
        crate::live_sync::sync_account_streamed(&self.client, account_id, cursors, progress, sink)
            .await
    }

    /// Lazily fetch the body content of a single message via `Email/get`.
    ///
    /// Bodies are not synced during metadata sync; they are fetched on first
    /// view and cached locally.
    ///
    /// @spec docs/L1-sync#sync-granularity
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        crate::live_message::fetch_message_body(self, message_id).await
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        crate::live_message::download_blob(self, blob_id).await
    }

    /// Add or remove keywords (flags) on a message via `Email/set`.
    ///
    /// Uses `ifInState` for optimistic concurrency when `expected_state` is provided.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::set_keywords(self, message_id, expected_state, command).await
    }

    /// Replace a message's mailbox membership via `Email/set`.
    ///
    /// Used for move and archive operations. Supports optimistic concurrency.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::replace_mailboxes(self, message_id, expected_state, mailbox_ids).await
    }

    /// Permanently destroy a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::destroy_message(self, message_id, expected_state).await
    }

    /// Update a mailbox role via `Mailbox/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::set_mailbox_role(
            self,
            mailbox_id,
            expected_state,
            role,
            clear_role_from,
        )
        .await
    }

    /// Fetch the primary sender identity for an account via `Identity/get`.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-compose#composesession-interface
    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        crate::live_compose::fetch_identity(self).await
    }

    /// Fetch the original message metadata needed for reply/forward composition.
    ///
    /// Retrieves subject, sender, recipients, threading headers, and quoted
    /// body text. The body is `>` prefixed for reply quoting.
    ///
    /// @spec docs/L1-compose#reply-quoting
    /// @spec docs/L1-compose#forward-quoting
    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        crate::live_compose::fetch_reply_context(self, message_id).await
    }

    /// Send a message via `Email/set` + `EmailSubmission/set` in a single JMAP request.
    ///
    /// Renders the Markdown body to HTML and constructs a multipart/alternative
    /// MIME structure. The server handles Sent folder placement.
    ///
    /// @spec docs/L1-compose#mime-structure
    /// @spec docs/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request_data: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        crate::live_compose::send_message(self, account_id, request_data).await
    }

    /// Persist a draft to the Drafts mailbox via `Email/set`, returning the
    /// created provider Email id. `replace` destroys the prior draft in the same
    /// request (JMAP emails are immutable; update = create-new + destroy-old).
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-jmap#methods-used
    async fn save_draft(
        &self,
        account_id: &AccountId,
        request_data: &SendMessageRequest,
        replace: Option<&MessageId>,
    ) -> Result<MessageId, GatewayError> {
        crate::live_compose::save_draft(self, account_id, request_data, replace).await
    }

    /// Destroy a draft message from the Drafts mailbox via `Email/set`.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-jmap#methods-used
    async fn delete_draft(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), GatewayError> {
        crate::live_compose::delete_draft(self, account_id, message_id).await
    }

    /// Return available push transports, preferring WebSocket over SSE.
    ///
    /// @spec docs/L2-transport#pushtransport
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        crate::live_push::push_transports(self)
    }
}
