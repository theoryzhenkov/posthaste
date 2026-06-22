use mail_parser::{Address, MessageParser};

use super::*;

impl MailService {
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

    /// Fetch compose-ready content for resuming an existing provider draft.
    ///
    /// Parses cached raw RFC822 bytes so Cc/Bcc are preserved. If the raw MIME
    /// is not cached yet and a gateway is available, the body is fetched and
    /// stored first, then parsed. As a last offline fallback, returns the fields
    /// available in [`MessageDetail`] (which cannot include Cc/Bcc).
    ///
    /// @spec docs/L1-outbox#operation-model
    pub async fn get_draft_content(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: Option<&dyn MailGateway>,
    ) -> Result<DraftContentResult, ServiceError> {
        let result = self
            .get_message_detail(account_id, message_id, gateway)
            .await?;
        let detail = result
            .detail
            .ok_or_else(|| StoreError::NotFound(format!("message {}", message_id.as_str())))?;
        let mut events = result.events;

        if let Some(raw) = self
            .message_detail_reader
            .read_raw_message(account_id, message_id)?
        {
            return Ok(DraftContentResult {
                content: draft_content_from_raw_mime(&raw)?,
                events,
            });
        }

        if let Some(gateway) = gateway {
            let fetched = gateway.fetch_message_body(account_id, message_id).await?;
            let cache_result = self
                .sync_writer
                .apply_message_body(account_id, message_id, &fetched)?;
            events.extend(cache_result.events);
            if let Some(raw) = self
                .message_detail_reader
                .read_raw_message(account_id, message_id)?
            {
                return Ok(DraftContentResult {
                    content: draft_content_from_raw_mime(&raw)?,
                    events,
                });
            }
        }

        Ok(DraftContentResult {
            content: DraftContent {
                from: detail.summary.from_email.map(|email| Recipient {
                    name: detail.summary.from_name,
                    email,
                }),
                to: detail.summary.to,
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: detail.summary.subject.unwrap_or_default(),
                body: detail.body_text.unwrap_or_default(),
            },
            events,
        })
    }

    /// Download a blob for a message, preferring already-cached raw bytes.
    ///
    /// When the message's raw RFC822 body is cached and the gateway can resolve
    /// the blob from it (IMAP), the bytes are served locally without a network
    /// round trip. Otherwise the blob is downloaded from the gateway.
    ///
    /// @spec docs/L1-sync#email-bodies-are-fetched-lazily
    pub async fn download_blob(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        blob_id: &crate::BlobId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<u8>, ServiceError> {
        if let Some(raw) = self
            .message_detail_reader
            .read_raw_message(account_id, message_id)?
        {
            if let Some(bytes) = gateway.extract_cached_blob(blob_id, &raw)? {
                return Ok(bytes);
            }
        }
        gateway
            .download_blob(account_id, blob_id)
            .await
            .map_err(Into::into)
    }
}

fn draft_content_from_raw_mime(raw_mime: &[u8]) -> Result<DraftContent, ServiceError> {
    let parsed = MessageParser::default()
        .parse(raw_mime)
        .ok_or_else(|| GatewayError::Rejected("cannot parse draft MIME".to_string()))?;
    Ok(DraftContent {
        from: parsed.from().and_then(first_recipient),
        to: parsed.to().map(addresses_to_recipients).unwrap_or_default(),
        cc: parsed.cc().map(addresses_to_recipients).unwrap_or_default(),
        bcc: parsed
            .bcc()
            .map(addresses_to_recipients)
            .unwrap_or_default(),
        subject: parsed.subject().unwrap_or_default().to_string(),
        body: parsed
            .body_text(0)
            .map(|body| body.to_string())
            .unwrap_or_default(),
    })
}

fn first_recipient(addresses: &Address<'_>) -> Option<Recipient> {
    addresses_to_recipients(addresses).into_iter().next()
}

fn addresses_to_recipients(addresses: &Address<'_>) -> Vec<Recipient> {
    addresses
        .iter()
        .filter_map(|address| {
            Some(Recipient {
                name: address.name.as_ref().map(|name| name.to_string()),
                email: address.address.as_ref()?.to_string(),
            })
        })
        .collect()
}
