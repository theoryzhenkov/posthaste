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

    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;
        let config = self.resolve_imap_config().await?;

        fetch_message_body_by_location(&config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        let (message_id, _attachment_index) =
            parse_imap_attachment_blob_id(blob_id).map_err(imap_error_to_gateway)?;
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, &message_id)?;
        let config = self.resolve_imap_config().await?;
        let raw_mime = fetch_raw_message_by_location(&config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)?;

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
        let config = self.resolve_imap_config().await?;

        apply_imap_keyword_delta_by_location(&config, &mailbox_name, &location, command)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let config = self.resolve_imap_config().await?;
        replace_message_mailboxes(self, &config, account_id, message_id, mailbox_ids).await
    }

    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let config = self.resolve_imap_config().await?;
        destroy_message_by_imap(self, &config, account_id, message_id).await
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
        let config = self.resolve_imap_config().await?;

        fetch_imap_reply_context_by_location(&config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
        idempotency_key: &str,
    ) -> Result<(), GatewayError> {
        let imap_config = self.resolve_imap_config().await?;
        let smtp_config = self.resolve_smtp_config().await?;
        send_message_via_smtp(self, &imap_config, &smtp_config, request, idempotency_key).await
    }

    async fn save_draft(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
        replace: Option<&MessageId>,
    ) -> Result<MessageId, GatewayError> {
        let config = self.resolve_imap_config().await?;
        save_imap_draft(self, &config, account_id, request, replace).await
    }

    async fn delete_draft(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), GatewayError> {
        let config = self.resolve_imap_config().await?;
        delete_imap_draft(self, &config, account_id, message_id).await
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}
