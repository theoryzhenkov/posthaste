use super::*;

#[async_trait]
impl MailGateway for LiveImapSmtpGateway {
    async fn sync(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        sync_imap_account(self, account_id, progress).await
    }

    /// Streamed, resumable sync (B4). Overrides the default single-emit so an
    /// interrupted INITIAL sync of a large mailbox checkpoints per UID chunk and
    /// resumes past the last committed UID on restart, rather than restarting
    /// from UID 1. See [`super::sync_imap_account_streamed`].
    async fn sync_streamed(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
        sink: &mut dyn SyncChunkSink,
    ) -> Result<SyncOutcome, GatewayError> {
        sync_imap_account_streamed(self, account_id, progress, sink).await
    }

    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;
        let mut lease = self
            .sessions
            .acquire("fetch_message_body")
            .await
            .map_err(imap_error_to_gateway)?;
        let result = fetch_message_body_by_location(lease.client(), &mailbox_name, &location).await;
        lease.finish(result).map_err(imap_error_to_gateway)
    }

    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        let (message_id, _attachment_index) =
            parse_imap_attachment_blob_id(blob_id).map_err(imap_error_to_gateway)?;
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, &message_id)?;
        let mut lease = self
            .sessions
            .acquire("download_blob")
            .await
            .map_err(imap_error_to_gateway)?;
        let result = fetch_raw_message_by_location(lease.client(), &mailbox_name, &location).await;
        let raw_mime = lease.finish(result).map_err(imap_error_to_gateway)?;

        imap_attachment_bytes_from_raw_mime(blob_id, raw_mime).map_err(imap_error_to_gateway)
    }

    fn extract_cached_blob(
        &self,
        blob_id: &BlobId,
        raw_mime: &[u8],
    ) -> Result<Option<Vec<u8>>, GatewayError> {
        imap_attachment_bytes_from_raw_mime(blob_id, raw_mime.to_vec())
            .map(Some)
            .map_err(imap_error_to_gateway)
    }

    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;
        let mut lease = self
            .sessions
            .acquire("set_keywords")
            .await
            .map_err(imap_error_to_gateway)?;
        let result =
            apply_imap_keyword_delta_by_location(lease.client(), &mailbox_name, &location, command)
                .await;
        lease.finish(result).map_err(imap_error_to_gateway)
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut lease = self
            .sessions
            .acquire("replace_mailboxes")
            .await
            .map_err(imap_error_to_gateway)?;
        let result =
            replace_message_mailboxes(self, lease.client(), account_id, message_id, mailbox_ids)
                .await;
        lease.finish_gateway(result)
    }

    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut lease = self
            .sessions
            .acquire("destroy_message")
            .await
            .map_err(imap_error_to_gateway)?;
        let result = destroy_message_by_imap(self, lease.client(), account_id, message_id).await;
        lease.finish_gateway(result)
    }

    async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        self.store("mailbox role override")?
            .set_mailbox_role_override(account_id, mailbox_id, role, clear_role_from)
            .map_err(store_error_to_gateway)?;

        Ok(MutationOutcome {
            cursor: None,
            message: None,
        })
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "imap-smtp-default".to_string(),
            name: self.smtp_config.sender_name.clone().unwrap_or_else(|| {
                self.smtp_config
                    .sender_email
                    .split('@')
                    .next()
                    .unwrap_or(self.smtp_config.sender_email.as_str())
                    .to_string()
            }),
            email: self.smtp_config.sender_email.clone(),
        })
    }

    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;
        let mut lease = self
            .sessions
            .acquire("fetch_reply_context")
            .await
            .map_err(imap_error_to_gateway)?;
        let result =
            fetch_imap_reply_context_by_location(lease.client(), &mailbox_name, &location).await;
        lease.finish(result).map_err(imap_error_to_gateway)
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
        idempotency_key: &str,
    ) -> Result<(), GatewayError> {
        let smtp_config = self.resolve_smtp_config().await?;
        send_message_via_smtp(self, &smtp_config, request, idempotency_key).await
    }

    async fn save_draft(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
        replace: Option<&MessageId>,
        // IMAP replaces a draft by APPEND-new + UID EXPUNGE-old: an absent old
        // draft simply matches no UID, so the idempotent-redelivery distinction
        // (DS3/D133) is a JMAP `Email/set` `notFound`-mask concern only and does
        // not change the IMAP path.
        _idempotent_redelivery: bool,
        // The deterministic create-id (DS2) is a JMAP create-with-id device; IMAP
        // APPEND has no create-id, so the stable X-Posthaste-Draft-Id header the
        // APPEND already stamps is the id that survives a redelivery.
        _idempotency_key: &str,
    ) -> Result<MessageId, GatewayError> {
        let mut lease = self
            .sessions
            .acquire("save_draft")
            .await
            .map_err(imap_error_to_gateway)?;
        let result = save_imap_draft(self, lease.client(), account_id, request, replace).await;
        lease.finish_gateway(result)
    }

    async fn delete_draft(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        // IMAP expunges by UID: an absent draft simply matches no UID, so the
        // idempotent-redelivery distinction (D133) is a JMAP `notFound`-mask
        // concern only and does not change the IMAP expunge path.
        _idempotent_redelivery: bool,
    ) -> Result<(), GatewayError> {
        let mut lease = self
            .sessions
            .acquire("delete_draft")
            .await
            .map_err(imap_error_to_gateway)?;
        let result = delete_imap_draft(self, lease.client(), account_id, message_id).await;
        lease.finish_gateway(result)
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}
