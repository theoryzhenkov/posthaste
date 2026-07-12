#[cfg(test)]
use std::collections::HashSet;

use mail_parser::{Address, MessageParser};
use posthaste_replica_core::{replay_message, MessageAssertion, MessageFoldState, MessageOutcome};

use super::*;

impl MailService {
    /// List messages, optionally filtered by mailbox.
    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        // Indexed SQL over the EFFECTIVE views (NS1: base ∪ overlay merged in
        // the store); the mailbox filter runs in SQL, no in-service fold.
        self.message_lister
            .list_messages(account_id, mailbox_id)
            .map_err(ServiceError::from)
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
        // Indexed SQL seek pagination over the EFFECTIVE views (NS1).
        self.message_lister
            .list_message_page(
                account_id,
                mailbox_id,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )
            .map_err(ServiceError::from)
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

    /// Fetch a single conversation with all its messages, folded over the
    /// read-time overlay so pending keyword/mailbox/destroy assertions are
    /// reflected. Thread membership is unaffected by mailbox/keyword changes,
    /// so folding each message summary in place is sufficient (a destroyed
    /// message drops out).
    ///
    /// Conversation-list envelope aggregates (unread/flagged counts) are a
    /// separate, harder case (re-derivation over folded membership) and are
    /// not folded here yet — they converge on the next sync.
    ///
    /// @spec docs/replication/L1#retire-on-confirmation
    pub fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, ServiceError> {
        let view = self
            .conversation_reader
            .get_conversation(conversation_id)?
            .not_found("conversation", conversation_id.as_str())?;
        // The store read serves the EFFECTIVE plane (NS1), so pending
        // assertions are already folded in — no in-service fold.
        Ok(view)
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

    /// Overlay-folded message detail WITHOUT the body: header + attachments, no
    /// body query and no gateway. The detail read surface uses this — the body
    /// is the separate `/body` lazy resource — so opening a message never loads
    /// the body just to drop it from the response.
    pub fn get_message_header(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, ServiceError> {
        // The store read serves the EFFECTIVE plane (NS1); no in-service fold.
        self.message_detail_reader
            .get_message_detail_without_body(account_id, message_id)
            .map_err(Into::into)
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
        let sync_writer = self.sync_writer.clone();
        let owned_account_id = account_id.clone();
        let owned_message_id = message_id.clone();
        offload(move || {
            sync_writer.apply_message_body(
                &crate::BaseWrite::reconciler(),
                &owned_account_id,
                &owned_message_id,
                &fetched,
            )
        })
        .await
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
        // NS2 Slice 3: a still-queued save is the FRESHEST content — serve it
        // straight from the op payload. Offline compose resume depends on
        // this: the instant-draft overlay row carries no body, and the
        // provider copy (if any) predates the queued edit.
        if let Some(content) = self.queued_draft_content(account_id, message_id)? {
            return Ok(DraftContentResult {
                content,
                events: Vec::new(),
            });
        }
        let result = self
            .get_message_detail(account_id, message_id, gateway)
            .await?;
        let detail = result
            .detail
            .ok_or_else(|| StoreError::NotFound(format!("message {}", message_id.as_str())))?;
        let mut events = result.events;
        // The stable draft identity is projected from the `X-Posthaste-Draft-Id`
        // header during sync; surface it so the client keys autosave by it and a
        // resumed edit updates the draft in place across provider id rotation.
        let draft_id = detail.summary.draft_id.clone();

        if let Some(raw) = self
            .message_detail_reader
            .read_raw_message(account_id, message_id)?
        {
            let mut content = draft_content_from_raw_mime(&raw)?;
            content.draft_id = draft_id;
            return Ok(DraftContentResult { content, events });
        }

        if let Some(gateway) = gateway {
            let fetched = gateway.fetch_message_body(account_id, message_id).await?;
            let sync_writer = self.sync_writer.clone();
            let owned_account_id = account_id.clone();
            let owned_message_id = message_id.clone();
            let cache_result = offload(move || {
                sync_writer.apply_message_body(
                    &crate::BaseWrite::reconciler(),
                    &owned_account_id,
                    &owned_message_id,
                    &fetched,
                )
            })
            .await?;
            events.extend(cache_result.events);
            if let Some(raw) = self
                .message_detail_reader
                .read_raw_message(account_id, message_id)?
            {
                let mut content = draft_content_from_raw_mime(&raw)?;
                content.draft_id = draft_id;
                return Ok(DraftContentResult { content, events });
            }
        }

        Ok(DraftContentResult {
            content: DraftContent {
                from: detail.summary.from_email.map(|email| Recipient {
                    name: detail.summary.from_name,
                    email,
                }),
                to: detail.summary.to,
                subject: detail.summary.subject.unwrap_or_default(),
                body: detail.body_text.unwrap_or_default(),
                draft_id,
                ..Default::default()
            },
            events,
        })
    }

    /// The content of the newest still-queued save for the draft `message_id`
    /// names, decoded from the op payload — `None` when no save is queued
    /// (the projection/raw-MIME paths serve then). `message_id` may be the
    /// live row id (the list row the user clicked) or the stable key itself;
    /// the projected `draft_id` bridges the two.
    fn queued_draft_content(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<DraftContent>, ServiceError> {
        let key = self
            .message_detail_reader
            .get_message_summary(account_id, message_id)?
            .and_then(|summary| summary.draft_id)
            .unwrap_or_else(|| message_id.to_string());
        let newest_save = self
            .outbox
            .list_unsettled_operations(account_id)?
            .into_iter()
            .rfind(|operation| {
                operation.entity.kind == OperationEntityKind::Draft
                    && operation.entity.id == key
                    && operation.kind.is_draft_save()
                    && matches!(
                        operation.state,
                        OperationState::Pending
                            | OperationState::Inflight
                            | OperationState::Applied
                    )
            });
        let Some(operation) = newest_save else {
            return Ok(None);
        };
        let Ok(posthaste_domain_model::MailIntent::SaveDraft { request, .. }) = operation.intent()
        else {
            return Ok(None);
        };
        Ok(Some(DraftContent {
            from: request.from,
            to: request.to,
            cc: request.cc,
            bcc: request.bcc,
            subject: request.subject,
            body: request.body,
            draft_id: Some(key),
        }))
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
        blob_id: &posthaste_domain_model::BlobId,
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

    /// Test-only since the NS1 cutover: production overlay maintenance filters
    /// inline in `mutation::refresh_message_overlay` (the one lifecycle
    /// function); this projection is retained for lifecycle-assertion tests.
    #[cfg(test)]
    pub(crate) fn overlay_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, ServiceError> {
        self.outbox
            .list_unsettled_operations(account_id)
            .map_err(ServiceError::from)
            .map(|operations| {
                operations
                    .into_iter()
                    .filter(|operation| {
                        operation.entity.kind == OperationEntityKind::Message
                            && operation.kind.is_state_assertion()
                            && matches!(
                                operation.state,
                                OperationState::Pending
                                    | OperationState::Inflight
                                    | OperationState::Applied
                            )
                    })
                    .collect()
            })
    }

    /// The ids of messages with an unsettled state-assertion op — the messages
    /// whose un-acked optimistic effect the M35 durable snapshot guard re-layers
    /// over provider truth (their canonical row holds an in-flight write the
    /// snapshot's view may predate). The sync path itself uses
    /// [`Self::overlay_operations`] directly (it needs the ops to fold, not just
    /// the ids); this id projection is retained for tests asserting the set.
    ///
    /// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    #[cfg(test)]
    pub(crate) fn unsettled_message_ids(
        &self,
        account_id: &AccountId,
    ) -> Result<HashSet<String>, ServiceError> {
        Ok(self
            .overlay_operations(account_id)?
            .into_iter()
            .map(|operation| operation.entity.id)
            .collect())
    }
}

/// Fold an account's pending message assertions over one summary using the
/// shared predictor ([posthaste_replica_core]). The renderer-facing read shape
/// (`MessageSummary`) is mapped onto the predictor's minimal canonical state,
/// folded, and mapped back, so the *effect* is defined once and is identical to
/// the one the WASM replica runs (`single-local-effect`).
///
/// @spec docs/replication/client-link/L2#1-the-shared-predictor-crate-posthaste-replica-core
/// Derive the ordered message assertions a message id's operations assert — the
/// single op→assertion mapping shared by the read overlay
/// ([`apply_operations_to_summary`]) and the settle write-back
/// ([`project_record`]).
///
/// @spec docs/eph/DESIGN-L2-optimistic-projection#4-canonical-vocabulary
pub(crate) fn message_assertions(
    operations: &[Operation],
    message_id: &str,
) -> Result<Vec<MessageAssertion>, ServiceError> {
    let mut assertions = Vec::new();
    for operation in operations
        .iter()
        .filter(|operation| operation.entity.id == message_id)
    {
        if let Some(FoldEffect::Assert(assertion)) = intent_fold_effect(operation)? {
            assertions.push(assertion);
        }
    }
    Ok(assertions)
}

/// The fold effect a typed intent contributes to the overlay plane (NS2
/// D171/D172): what becomes VISIBLE, decided from the gesture + the replicated
/// plane only. `Send` yields `None` until Slice 4 lands its multi-row effects.
pub(crate) enum FoldEffect {
    /// A message state assertion, replayed by the shared predictor.
    Assert(MessageAssertion),
    /// A queued draft save: the overlay upserts a row synthesized from the
    /// request (instant drafts — the row exists the moment the op is queued).
    UpsertDraft(posthaste_domain_model::SendMessageRequest),
    /// A queued draft removal: the overlay tombstones the draft's live row.
    TombstoneDraft,
    /// A queued send (NS2 Slice 4, D172): multi-row — the consumed draft's
    /// live row tombstones and a provisional Sent row upserts at the send's
    /// own entity id. Phase-aware in the engine: a HELD send folds neither
    /// (the draft stays visible and cancelable); a due/dispatched send folds
    /// both.
    SendEffects {
        /// The materialized originating-draft key (D170), when this send
        /// consumes one.
        consumes_draft_key: Option<String>,
        request: posthaste_domain_model::SendMessageRequest,
    },
}

/// NS2 Slices 2+3: THE effect interpreter for the overlay fold — the one place
/// a typed intent maps to its fold effect (D172: effects are total — folding a
/// tombstone over a nonexistent row is a no-op downstream).
pub(crate) fn intent_fold_effect(
    operation: &Operation,
) -> Result<Option<FoldEffect>, ServiceError> {
    let intent = operation.intent().map_err(|error| {
        ServiceError::from(posthaste_domain_model::GatewayError::Internal(error))
    })?;
    Ok(match intent {
        posthaste_domain_model::MailIntent::SetKeywords(command) => {
            Some(FoldEffect::Assert(MessageAssertion::SetKeywords {
                add: command.add,
                remove: command.remove,
            }))
        }
        posthaste_domain_model::MailIntent::ReplaceMailboxes(command) => {
            Some(FoldEffect::Assert(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: command
                    .mailbox_ids
                    .into_iter()
                    .map(|mailbox_id| mailbox_id.0)
                    .collect(),
            }))
        }
        posthaste_domain_model::MailIntent::Destroy => {
            Some(FoldEffect::Assert(MessageAssertion::Destroy))
        }
        posthaste_domain_model::MailIntent::SaveDraft { request, .. } => {
            Some(FoldEffect::UpsertDraft(request))
        }
        posthaste_domain_model::MailIntent::DiscardDraft { .. } => Some(FoldEffect::TombstoneDraft),
        posthaste_domain_model::MailIntent::Send(request) => {
            let consumes_draft_key = request
                .draft_id
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_string);
            Some(FoldEffect::SendEffects {
                consumes_draft_key,
                request,
            })
        }
    })
}

/// Settle write-back: fold the still-unsettled assertions over a provider
/// readback record (the new base) and yield the canonical row to persist, or
/// `None` when the message folds to removed. Uses the same `replay_message` the
/// read overlay uses; `is_read`/`is_flagged` are derived from keywords on the
/// store write.
///
/// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
pub(crate) fn project_record(
    mut record: posthaste_domain_model::MessageRecord,
    remaining_ops: &[Operation],
) -> Result<Option<posthaste_domain_model::MessageRecord>, ServiceError> {
    let assertions = message_assertions(remaining_ops, record.id.as_str())?;
    let base = MessageFoldState {
        keywords: std::mem::take(&mut record.keywords),
        mailbox_ids: record
            .mailbox_ids
            .iter()
            .map(|mailbox_id| mailbox_id.0.clone())
            .collect(),
    };
    match replay_message(base, &assertions) {
        MessageOutcome::Removed => Ok(None),
        MessageOutcome::Present(state) => {
            record.keywords = state.keywords;
            record.mailbox_ids = state.mailbox_ids.into_iter().map(MailboxId).collect();
            Ok(Some(record))
        }
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
        // Filled in by the caller from the projected draft identity.
        draft_id: None,
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
