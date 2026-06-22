use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use mail_parser::{Address, MessageParser};

use super::*;

/// Overlay-induced change to a single mailbox's `(unread, total)` counts.
///
/// @spec docs/L1-outbox#overlay-fold
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MailboxCountDelta {
    pub(crate) unread: i64,
    pub(crate) total: i64,
}

impl MailService {
    /// List messages, optionally filtered by mailbox.
    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let summaries = self
            .message_lister
            .list_messages(account_id, None)
            .map_err(ServiceError::from)?;
        self.fold_message_overlay(account_id, summaries, mailbox_id)
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
        let mut items = self.list_messages(account_id, mailbox_id)?;
        sort_message_summaries(&mut items, sort_field, sort_direction);
        let start = cursor
            .and_then(|cursor| {
                items.iter().position(|item| {
                    item.source_id == cursor.source_id && item.id == cursor.message_id
                })
            })
            .map_or(0, |index| index + 1);
        let page_items = items
            .iter()
            .skip(start)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = if start + page_items.len() < items.len() {
            page_items
                .last()
                .map(|item| message_cursor(item, sort_field))
        } else {
            None
        };
        Ok(MessagePage {
            items: page_items,
            next_cursor,
        })
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
            .and_then(|detail| self.apply_message_overlay(account_id, detail).transpose())
            .transpose()?
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

    /// Fold pending message-state assertions over synced summaries and then
    /// apply the requested mailbox filter to the folded view.
    ///
    /// @spec docs/L1-outbox#overlay-fold
    pub(crate) fn fold_message_overlay(
        &self,
        account_id: &AccountId,
        summaries: Vec<MessageSummary>,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let operations = self.overlay_operations(account_id)?;
        let mut folded = Vec::new();
        for summary in summaries {
            let Some(summary) = apply_operations_to_summary(summary, &operations)? else {
                continue;
            };
            if mailbox_id.is_none_or(|mailbox_id| summary.mailbox_ids.contains(mailbox_id)) {
                folded.push(summary);
            }
        }
        Ok(folded)
    }

    /// Fold pending message-state assertions over a detail view.
    ///
    /// @spec docs/L1-outbox#overlay-fold
    pub(crate) fn apply_message_overlay(
        &self,
        account_id: &AccountId,
        mut detail: MessageDetail,
    ) -> Result<Option<MessageDetail>, ServiceError> {
        let operations = self.overlay_operations(account_id)?;
        let Some(summary) = apply_operations_to_summary(detail.summary, &operations)? else {
            return Ok(None);
        };
        detail.summary = summary;
        Ok(Some(detail))
    }

    /// Per-mailbox count adjustments implied by pending message assertions.
    ///
    /// Returns the delta to apply to each mailbox's stored `(unread, total)`
    /// counts so sidebar counts reflect the read-time overlay. Bounded by the
    /// number of messages with pending assertions, and empty when there is no
    /// pending work.
    ///
    /// @spec docs/L1-outbox#overlay-fold
    pub(crate) fn mailbox_count_overlay(
        &self,
        account_id: &AccountId,
    ) -> Result<BTreeMap<MailboxId, MailboxCountDelta>, ServiceError> {
        let operations = self.overlay_operations(account_id)?;
        if operations.is_empty() {
            return Ok(BTreeMap::new());
        }
        let affected: BTreeSet<&str> = operations.iter().map(|op| op.entity.id.as_str()).collect();
        let mut deltas: BTreeMap<MailboxId, MailboxCountDelta> = BTreeMap::new();
        let base_summaries = self
            .message_lister
            .list_messages(account_id, None)
            .map_err(ServiceError::from)?;
        for base in base_summaries
            .into_iter()
            .filter(|summary| affected.contains(summary.id.as_str()))
        {
            for mailbox_id in &base.mailbox_ids {
                let delta = deltas.entry(mailbox_id.clone()).or_default();
                delta.total -= 1;
                if !base.is_read {
                    delta.unread -= 1;
                }
            }
            if let Some(folded) = apply_operations_to_summary(base, &operations)? {
                for mailbox_id in &folded.mailbox_ids {
                    let delta = deltas.entry(mailbox_id.clone()).or_default();
                    delta.total += 1;
                    if !folded.is_read {
                        delta.unread += 1;
                    }
                }
            }
        }
        deltas.retain(|_, delta| delta.unread != 0 || delta.total != 0);
        Ok(deltas)
    }

    pub(crate) fn overlay_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, ServiceError> {
        self.outbox
            .list_pending_operations(account_id)
            .map_err(ServiceError::from)
            .map(|operations| {
                operations
                    .into_iter()
                    .filter(|operation| {
                        operation.entity.kind == OperationEntityKind::Message
                            && operation.kind.is_state_assertion()
                            && matches!(
                                operation.state,
                                OperationState::Pending | OperationState::Inflight
                            )
                    })
                    .collect()
            })
    }
}

fn apply_operations_to_summary(
    mut summary: MessageSummary,
    operations: &[Operation],
) -> Result<Option<MessageSummary>, ServiceError> {
    for operation in operations
        .iter()
        .filter(|operation| operation.entity.id == summary.id.as_str())
    {
        match operation.kind {
            OperationKind::SetKeywords => {
                let command =
                    serde_json::from_value::<SetKeywordsCommand>(operation.payload.clone())
                        .map_err(|error| {
                            ServiceError::from(GatewayError::Rejected(format!(
                                "invalid setKeywords overlay payload: {error}"
                            )))
                        })?;
                let mut keywords = summary.keywords.into_iter().collect::<BTreeSet<_>>();
                for keyword in command.add {
                    keywords.insert(keyword);
                }
                for keyword in command.remove {
                    keywords.remove(&keyword);
                }
                summary.keywords = keywords.into_iter().collect();
                summary.is_read = summary.keywords.iter().any(|keyword| keyword == "$seen");
                summary.is_flagged = summary.keywords.iter().any(|keyword| keyword == "$flagged");
            }
            OperationKind::ReplaceMailboxes => {
                let command =
                    serde_json::from_value::<ReplaceMailboxesCommand>(operation.payload.clone())
                        .map_err(|error| {
                            ServiceError::from(GatewayError::Rejected(format!(
                                "invalid replaceMailboxes overlay payload: {error}"
                            )))
                        })?;
                summary.mailbox_ids = command.mailbox_ids;
            }
            OperationKind::Destroy => return Ok(None),
            _ => {}
        }
    }
    Ok(Some(summary))
}

pub(crate) fn sort_message_summaries(
    summaries: &mut [MessageSummary],
    sort_field: MessageSortField,
    sort_direction: SortDirection,
) {
    summaries.sort_by(|left, right| {
        let ordering = compare_message_summary(left, right, sort_field);
        let ordering = match sort_direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        };
        ordering
            .then_with(|| left.source_id.as_str().cmp(right.source_id.as_str()))
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
}

fn compare_message_summary(
    left: &MessageSummary,
    right: &MessageSummary,
    sort_field: MessageSortField,
) -> Ordering {
    match sort_field {
        MessageSortField::Date => left.received_at.cmp(&right.received_at),
        MessageSortField::From => {
            sort_string(left.from_name.as_deref().or(left.from_email.as_deref())).cmp(&sort_string(
                right.from_name.as_deref().or(right.from_email.as_deref()),
            ))
        }
        MessageSortField::Subject => {
            sort_string(left.subject.as_deref()).cmp(&sort_string(right.subject.as_deref()))
        }
        MessageSortField::Source => sort_string(Some(left.source_name.as_str()))
            .cmp(&sort_string(Some(right.source_name.as_str()))),
        MessageSortField::Flagged => left.is_flagged.cmp(&right.is_flagged),
        MessageSortField::Attachment => left.has_attachment.cmp(&right.has_attachment),
    }
}

fn sort_string(value: Option<&str>) -> String {
    value.unwrap_or_default().to_lowercase()
}

pub(crate) fn message_cursor(
    summary: &MessageSummary,
    sort_field: MessageSortField,
) -> MessageCursor {
    let sort_value = match sort_field {
        MessageSortField::Date => summary.received_at.clone(),
        MessageSortField::From => sort_string(
            summary
                .from_name
                .as_deref()
                .or(summary.from_email.as_deref()),
        ),
        MessageSortField::Subject => sort_string(summary.subject.as_deref()),
        MessageSortField::Source => sort_string(Some(summary.source_name.as_str())),
        MessageSortField::Flagged => i32::from(summary.is_flagged).to_string(),
        MessageSortField::Attachment => i32::from(summary.has_attachment).to_string(),
    };
    MessageCursor {
        sort_value,
        source_id: summary.source_id.clone(),
        message_id: summary.id.clone(),
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
