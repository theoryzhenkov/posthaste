//! Draft lifecycle: local-first save/delete, the user-initiated optimistic
//! discard (D130), the send-consume settlement effect (D126), and flush-time
//! stable-key -> live-id resolution (M70/D136).

use super::classify::FlushError;
use crate::service::*;
use posthaste_domain_model::CommandAck;

impl MailService {
    /// Save a draft local-first: enqueue a draft create/update operation.
    ///
    /// `draft_id` is `None` for a brand-new draft (a temporary entity id is
    /// minted and reconciled to the provider id on first flush) or the existing
    /// draft's id for an edit. The optimistic draft lives in the outbox until it
    /// flushes to the provider's Drafts mailbox; consumers render pending draft
    /// operations to show it while offline.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-outbox#temp-id-reconciliation
    pub fn save_draft(
        &self,
        account_id: &AccountId,
        draft_key: Option<MessageId>,
        mut request: SendMessageRequest,
    ) -> Result<Operation, ServiceError> {
        // `draft_key` is a stable, client-chosen handle for the draft. The
        // registry maps it to the entity id currently representing the draft (a
        // temporary id before the first flush, a provider id after), so the
        // client can keep using the same key across flushes without creating
        // duplicate drafts.
        let key = draft_key
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("draft-local-{}", Id::generate()));
        // Stamp the stable identity into the payload so the gateway writes it as
        // the `X-Posthaste-Draft-Id` header. The header survives the provider id
        // rotation a JMAP draft update causes and is read back on resume, so the
        // client always keys by a stable value.
        request.draft_id = Some(key.clone());
        // M70 (D136): the op carries the STABLE key as its entity id — the live
        // id is resolved at flush, immediately before the gateway call, so the
        // push always targets the freshest mapping the registry knows (a
        // rotation observed between enqueue and flush cannot stale it).
        // Enqueue-time resolution only picks the kind: a key the registry knows
        // is an update; an unknown key registers itself (a self-mapping until
        // the first flush assigns a provider id).
        let kind = match self.draft_registry.resolve_draft_entity(account_id, &key)? {
            Some(_) => OperationKind::DraftUpdate,
            None => {
                self.draft_registry
                    .set_draft_alias(account_id, &key, &key)?;
                // A key with no registry row that already names an existing
                // draft message is a draft resumed by its (rotating) provider
                // id — a legacy draft saved before stable ids, or one created
                // elsewhere. Replace it in place instead of creating a
                // duplicate; this also bootstraps a stable header onto it.
                if self.draft_message_exists(account_id, &key)? {
                    OperationKind::DraftUpdate
                } else {
                    OperationKind::DraftCreate
                }
            }
        };
        let payload = encode_payload(request, "draft request")?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: key,
            },
            kind,
            payload,
            None,
            None,
        )
    }

    /// Delete a draft local-first: enqueue a draft delete operation carrying
    /// the draft's STABLE key (M70/D136). The live entity id (temporary or
    /// provider-assigned) is resolved at flush, immediately before the provider
    /// destroy, so a rotation observed between enqueue and flush (an in-flight
    /// save settling, a sync-observed other-device edit repointing the
    /// registry) retargets the destroy to the current live draft.
    ///
    /// The registry mapping is NOT forgotten here (M70): identity survives
    /// until the destruction is confirmed — at this op's settlement (the
    /// `DraftDelete` Applied arm of [`Self::flush_pass`]) or at sync-observed
    /// disappearance (M69's confirmed-gone prune) — so an in-flight op never
    /// references a forgotten mapping.
    ///
    /// `idempotent_redelivery` records whether a provider `notFound` at flush
    /// time is a benign already-gone (the send-consume settlement effect, which
    /// re-enqueues the delete under redelivery — D126) or a genuine failure a
    /// user-initiated discard must surface (D133). It is stamped onto the op so
    /// the gateway narrows its `notFound ⇒ Ok` mask to the idempotent case only.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    pub fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
        idempotent_redelivery: bool,
    ) -> Result<Operation, ServiceError> {
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: draft_key.to_string(),
            },
            OperationKind::DraftDelete,
            serde_json::json!({ "idempotentRedelivery": idempotent_redelivery }),
            None,
            None,
        )
    }

    /// Discard a draft through the optimistic runtime-mutation path (D130).
    ///
    /// Unlike [`delete_draft`] (the send-consume settlement effect), this is
    /// user-initiated: it resolves the stable draft key to the live entity
    /// (D131), removes the local draft row and emits the reconciling
    /// `message.updated{deleted:true}` immediately so the client's base prunes
    /// and the optimistic destroy retires without a follow-up sync (D132), and
    /// queues the provider destroy as a NON-idempotent op (D133 — a `notFound`
    /// now surfaces retryably). A key that no longer names a live draft is a
    /// surfaced `NotFound`, not a silent success: the client reverts the
    /// optimistic fold and shows the error (D133/D134).
    ///
    /// @spec docs/eph/RFC-L2-drafts#rfc-part-2
    pub async fn discard_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
    ) -> Result<CommandAck, ServiceError> {
        let key = draft_key.to_string();
        let entity_id = self
            .draft_registry
            .resolve_draft_entity(account_id, &key)?
            .unwrap_or_else(|| key.clone());
        let message_id = MessageId::from(entity_id.as_str());
        // A discard of a draft that no longer resolves to a live local row must
        // surface (D133) — the optimistic fold on the client reverts + shows the
        // error rather than silently "succeeding".
        let mailboxes = self
            .message_mailboxes
            .get_message_mailboxes(account_id, &message_id)?;
        if mailboxes.is_empty()
            && self
                .message_detail_reader
                .get_message_summary(account_id, &message_id)?
                .is_none()
        {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "draft:{}",
                message_id.as_str()
            ))));
        }
        // Queue the provider destroy (non-idempotent) via the stable-id path.
        let operation = self.delete_draft(account_id, draft_key, false)?;
        // THE LAST NON-RECONCILER BASE WRITER (NS1b): the draft-discard
        // optimistic destroy still writes base directly — entity-op territory
        // that cuts over with NS2 (send/draft as single intents), deleting
        // this call and its legacy grant with it. On a local failure, retract
        // the op so the outbox and canonical do not diverge.
        let message_commands = self.message_commands.clone();
        let owned_account = account_id.clone();
        let owned_message = message_id.clone();
        if let Err(error) = offload(move || {
            message_commands.destroy_message(
                &crate::BaseWrite::legacy("NS2 pending: draft-discard optimistic destroy"),
                &owned_account,
                &owned_message,
                None,
            )
        })
        .await
        {
            let _ = self.outbox.remove_operation(&operation.id);
            return Err(ServiceError::from(error));
        }
        let event = match self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            mailboxes.first(),
            Some(&message_id),
            serde_json::json!({ "messageId": message_id.as_str(), "deleted": true }),
        ) {
            Ok(event) => event,
            Err(error) => {
                let _ = self.outbox.remove_operation(&operation.id);
                return Err(ServiceError::from(error));
            }
        };
        Ok(CommandAck {
            events: vec![event],
        })
    }

    /// Whether `draft_key` names a message already in the projection — i.e. a
    /// draft being resumed by its provider id rather than a freshly minted
    /// local key. Used to edit such a draft in place instead of duplicating it.
    /// Mailbox membership is the light existence proxy: a draft always sits in
    /// the Drafts mailbox.
    fn draft_message_exists(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<bool, ServiceError> {
        let message_id = MessageId::from(draft_key);
        Ok(!self
            .message_mailboxes
            .get_message_mailboxes(account_id, &message_id)?
            .is_empty())
    }

    /// D126: draft destruction is a settlement effect of the send. When a
    /// settled-successful `Send` carries the originating draft's stable id
    /// (`SendMessageRequest::draft_id`), enqueue the draft's delete so the
    /// consumed draft leaves the provider's Drafts mailbox. Enqueued — not
    /// pushed inline — so a transient destroy failure is retried with the
    /// outbox/settlement machinery, never silent, and never re-runs the send.
    ///
    /// Idempotent across settlement redelivery (ruling 24): once the consumed
    /// draft's destroy settles, its registry mapping is forgotten (M70 —
    /// settlement-time forget), and the gateways treat an already-gone draft as
    /// destroyed, so a redelivered send settlement enqueues nothing (unknown
    /// draft) or settles an at-worst harmless second delete.
    ///
    /// On a parked send (`DispatchUncertain`) this is never reached — the draft
    /// is KEPT as the user's recovery artifact (D125); destruction happens only
    /// on settled success.
    ///
    /// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
    pub(super) fn consume_draft_after_send(
        &self,
        account_id: &AccountId,
        operation: &Operation,
    ) -> Result<Option<Operation>, ServiceError> {
        if operation.kind != OperationKind::Send {
            return Ok(None);
        }
        // The payload decoded to push the send, so a failure here is
        // unreachable in practice; it must not un-settle the settled send.
        let Ok(posthaste_domain_model::MailIntent::Send(request)) = operation.intent().map_err(|_| ()).and_then(|intent| match intent {
            posthaste_domain_model::MailIntent::Send(_) => Ok(intent),
            _ => Err(()),
        })
        else {
            return Ok(None);
        };
        let Some(key) = request
            .draft_id
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            return Ok(None);
        };
        // A key that resolves to no alias and no projected message names a
        // draft already consumed (redelivery) or never saved — nothing to do.
        let known = self
            .draft_registry
            .resolve_draft_entity(account_id, key)?
            .is_some()
            || self.draft_message_exists(account_id, key)?;
        if !known {
            return Ok(None);
        }
        self.delete_draft(account_id, MessageId::from(key), true)
            .map(Some)
    }

    /// M70 (D136): resolve a draft op's stable key to the live entity id at
    /// flush time — immediately before the gateway call — so the push targets
    /// the freshest mapping the registry knows. This closes the in-flight-op
    /// vs sync race M69 flagged: a sync chunk that repointed the registry (a
    /// rotation observed from another device) between enqueue and flush is
    /// reflected in the target. A key the registry no longer knows falls back
    /// to itself — the pre-M70 enqueue-time semantics, preserved so the
    /// provider still surfaces `notFound` per D133 (the uniform already-gone
    /// reading is M71/D137, for which this freshness guarantee is the
    /// prerequisite).
    ///
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    pub(super) fn resolve_draft_flush_target(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<String, FlushError> {
        Ok(self
            .draft_registry
            .resolve_draft_entity(account_id, draft_key)
            .map_err(|error| {
                FlushError::permanent(format!("draft identity resolution failed: {error}"))
            })?
            .unwrap_or_else(|| draft_key.to_string()))
    }
}
