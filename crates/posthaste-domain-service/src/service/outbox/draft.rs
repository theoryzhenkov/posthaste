//! Draft lifecycle (NS2 Slice 3): local-first save/discard as typed intents
//! whose fold effects land in the OVERLAY plane — a queued save is a visible
//! draft row immediately (no provider round trip, no sync lag), a queued
//! discard hides the row immediately, and base stays sync-owned (the last
//! non-reconciler base writer died with this cutover). Same-key saves
//! COALESCE (D174, last-writer-wins per compose session); flush-time
//! stable-key → live-id resolution stays the one registry seam (M70/D136).

use super::classify::FlushError;
use crate::service::mutation::{synthesize_draft_record, OverlayRetire};
use crate::service::*;
use posthaste_domain_model::CommandAck;

impl MailService {
    /// Save a draft local-first: enqueue (or coalesce into) a draft
    /// create/update operation, fold it into the overlay plane, and emit the
    /// projection echo — the draft appears in Drafts the moment this returns.
    ///
    /// `draft_key` is `None` for a brand-new draft (a stable local key is
    /// minted) or the draft's stable key for an edit. D174: a save whose key
    /// already has a still-queued save REPLACES that op's payload in place
    /// (same op id — the create idempotency identity — and kind), so the
    /// outbox holds at most one queued save per compose session and
    /// `depends_on` chains are gone.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-outbox#temp-id-reconciliation
    pub async fn save_draft(
        &self,
        account_id: &AccountId,
        draft_key: Option<MessageId>,
        mut request: SendMessageRequest,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
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
        // Enqueue-time resolution only picks the kind, and REGISTERS the key
        // (reserve-at-admission, D153): an unknown key self-maps until the
        // first flush assigns a provider id.
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
        // D174 coalescing: replace the still-queued save's payload in place.
        // The guarded swap races the flusher's claim with exactly one winner —
        // a claimed (inflight) save is never rewritten mid-push; the loser
        // falls through and enqueues a fresh op.
        let mut coalesced: Option<Operation> = None;
        if let Some(queued) = self
            .outbox
            .list_pending_operations(account_id)?
            .into_iter()
            .rfind(|existing| {
                existing.entity.kind == OperationEntityKind::Draft
                    && existing.entity.id == key
                    && existing.kind.is_draft_save()
                    && existing.state == OperationState::Pending
            })
        {
            if self
                .outbox
                .replace_operation_payload(&queued.id, &payload)?
            {
                coalesced = self.outbox.get_operation(&queued.id)?;
            }
        }
        let operation = match coalesced {
            Some(operation) => operation,
            None => self.queue_operation(
                account_id,
                OperationEntity {
                    kind: OperationEntityKind::Draft,
                    id: key.clone(),
                },
                kind,
                payload,
                None,
                None,
            )?,
        };
        // Instant draft: fold the queued save into the overlay plane and echo
        // the projection from the effective read, exactly like a message
        // assertion — the Drafts list row exists before any provider call.
        let live_id = self.live_draft_id(account_id, &key)?;
        self.refresh_message_overlay(account_id, &live_id, OverlayRetire::Immediate)
            .await?;
        let mut events = Vec::new();
        if let Some(summary) = self
            .message_detail_reader
            .get_message_summary(account_id, &live_id)?
        {
            let scope = summary.mailbox_ids.first().cloned();
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                scope.as_ref(),
                Some(&live_id),
                serde_json::json!({
                    "messageId": live_id.as_str(),
                    "changes": { "mailboxes": true },
                    "projection": &summary,
                }),
            )?);
        }
        Ok((operation, events))
    }

    /// Delete a draft local-first: enqueue a draft delete operation carrying
    /// the draft's STABLE key (M70/D136), fold the tombstone into the overlay
    /// (the row disappears immediately), and emit the reconciling deletion
    /// echo. The live entity id is resolved at flush, immediately before the
    /// provider destroy, so a rotation observed between enqueue and flush
    /// retargets the destroy to the current live draft.
    ///
    /// The registry mapping is NOT forgotten here (M70): identity survives
    /// until the destruction is confirmed — at this op's settlement or at
    /// sync-observed disappearance — so an in-flight op never references a
    /// forgotten mapping.
    ///
    /// `idempotent_redelivery` records whether a provider `notFound` at flush
    /// time is a benign already-gone (the send-consume settlement effect —
    /// D126) or a genuine failure a user-initiated discard must surface
    /// (D133). It is stamped onto the op so the gateway narrows its
    /// `notFound ⇒ Ok` mask to the idempotent case only.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    pub async fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
        idempotent_redelivery: bool,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
        let key = draft_key.to_string();
        // Reserve-at-admission (D153), delete half: a key the registry does
        // not know yet (a headerless legacy/foreign draft addressed by its
        // provider id) self-maps here, so the flush-time resolve ALWAYS finds
        // a mapping and a typed miss there can only mean confirmed
        // destruction — never "this key was simply never registered".
        if self
            .draft_registry
            .resolve_draft_entity(account_id, &key)?
            .is_none()
        {
            self.draft_registry
                .set_draft_alias(account_id, &key, &key)?;
        }
        let live_id = self.live_draft_id(account_id, &key)?;
        // Effective membership BEFORE the fold: the deletion echo's mailbox
        // scope, and the "was anything visible?" gate for emitting it.
        let previous = self
            .message_detail_reader
            .get_message_summary(account_id, &live_id)?;
        let operation = self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: key,
            },
            OperationKind::DraftDelete,
            serde_json::json!({ "idempotentRedelivery": idempotent_redelivery }),
            None,
            None,
        )?;
        // The tombstone fold: the draft leaves every effective view NOW; the
        // provider destroy follows through the outbox discipline.
        self.refresh_message_overlay(account_id, &live_id, OverlayRetire::Immediate)
            .await?;
        let mut events = Vec::new();
        if let Some(previous) = previous {
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                previous.mailbox_ids.first(),
                Some(&live_id),
                serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
            )?);
        }
        Ok((operation, events))
    }

    /// Discard a draft, user-initiated (D130/D133).
    ///
    /// D174/NS2: still-queued saves for the key are superseded (removed) —
    /// the discard wins the compose session. A draft that never reached the
    /// provider (no base row, no save ever in flight, never rotated) is
    /// discarded entirely locally: registry forgotten, overlay entry removed,
    /// no provider op at all. Otherwise the provider destroy is enqueued
    /// non-idempotent so a `notFound` surfaces (D133). A key that no longer
    /// names a live visible row is a surfaced `NotFound`, not a silent
    /// success. Base is untouched either way — the NS1 seal holds (the last
    /// `BaseWrite::legacy` production grant died with this rewrite).
    pub async fn discard_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
    ) -> Result<CommandAck, ServiceError> {
        let key = draft_key.to_string();
        let resolved = self.draft_registry.resolve_draft_entity(account_id, &key)?;
        let live = resolved.clone().unwrap_or_else(|| key.clone());
        let live_id = MessageId::from(live.as_str());
        // A discard of a draft with no live visible row must surface (D133) —
        // the client reverts the optimistic fold and shows the error. The
        // effective read covers both a synced draft and a queued-only one.
        let summary = self
            .message_detail_reader
            .get_message_summary(account_id, &live_id)?;
        if summary.is_none() {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "draft:{}",
                live_id.as_str()
            ))));
        }
        // Supersede queued saves: each removal is the same guarded
        // cancel-vs-flush statement the user-cancel path uses, so a save the
        // flusher claimed concurrently survives (and marks the provider as
        // possibly holding the draft).
        let mut save_may_have_reached_provider = false;
        for existing in self.outbox.list_pending_operations(account_id)? {
            if existing.entity.kind != OperationEntityKind::Draft
                || existing.entity.id != key
                || !existing.kind.is_draft_save()
            {
                continue;
            }
            match existing.state {
                OperationState::Pending | OperationState::Failed => {
                    if !self.outbox.remove_operation_unless_inflight(&existing.id)? {
                        save_may_have_reached_provider = true;
                    }
                }
                _ => save_may_have_reached_provider = true,
            }
        }
        let base_has_row = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_message = live_id.clone();
            offload(move || overlay.read_base_message_record(&owned_account, &owned_message))
                .await?
                .is_some()
        };
        let rotated = resolved.as_deref().is_some_and(|entity| entity != key);
        if base_has_row || save_may_have_reached_provider || rotated {
            // The provider (or an in-flight save) may hold the draft: queue
            // the non-idempotent destroy; the tombstone fold hides the row
            // immediately.
            let (_operation, events) = self.delete_draft(account_id, draft_key, false).await?;
            return Ok(CommandAck { events });
        }
        // Never reached the provider: a purely local discard. Forget the
        // reserved mapping, drop the overlay entry (no ops remain), echo the
        // deletion.
        self.draft_registry.remove_draft_alias(account_id, &key)?;
        self.refresh_message_overlay(account_id, &live_id, OverlayRetire::Immediate)
            .await?;
        let event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            summary
                .as_ref()
                .and_then(|summary| summary.mailbox_ids.first()),
            Some(&live_id),
            serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
        )?;
        Ok(CommandAck {
            events: vec![event],
        })
    }

    /// The live id a stable draft key currently maps to — the id its visible
    /// row is keyed by (the key itself until a flush assigns a provider id).
    fn live_draft_id(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<MessageId, ServiceError> {
        Ok(MessageId::from(
            self.draft_registry
                .resolve_draft_entity(account_id, draft_key)?
                .unwrap_or_else(|| draft_key.to_string())
                .as_str(),
        ))
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
    /// consumed draft leaves the provider's Drafts mailbox — and, since Slice
    /// 3, the local Drafts list immediately (the delete's tombstone fold).
    /// Enqueued — not pushed inline — so a transient destroy failure is
    /// retried with the outbox/settlement machinery, never silent, and never
    /// re-runs the send. Returns whether a follow-up op was enqueued.
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
    pub(super) async fn consume_draft_after_send(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        events: &mut Vec<DomainEvent>,
    ) -> Result<bool, ServiceError> {
        if operation.kind != OperationKind::Send {
            return Ok(false);
        }
        // The payload decoded to push the send, so a failure here is
        // unreachable in practice; it must not un-settle the settled send.
        let Ok(posthaste_domain_model::MailIntent::Send(request)) = operation.intent() else {
            return Ok(false);
        };
        let Some(key) = request
            .draft_id
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            return Ok(false);
        };
        // A key that resolves to no alias and no projected message names a
        // draft already consumed (redelivery) or never saved — nothing to do.
        let known = self
            .draft_registry
            .resolve_draft_entity(account_id, key)?
            .is_some()
            || self.draft_message_exists(account_id, key)?;
        if !known {
            return Ok(false);
        }
        let (_operation, delete_events) = self
            .delete_draft(account_id, MessageId::from(key), true)
            .await?;
        events.extend(delete_events);
        Ok(true)
    }

    /// M70 (D136): resolve a draft op's stable key to the live entity id at
    /// flush time — immediately before the gateway call — so the push targets
    /// the freshest mapping the registry knows. This closes the in-flight-op
    /// vs sync race M69 flagged: a sync chunk that repointed the registry (a
    /// rotation observed from another device) between enqueue and flush is
    /// reflected in the target.
    ///
    /// `None` is the TYPED miss (D153, replacing the old silent
    /// `unwrap_or_else(key)` fallback): the registry forgets only on CONFIRMED
    /// destruction, so a miss means the draft is gone — the caller decides
    /// what its op means then (a save re-creates; a discard settles as
    /// already-done).
    ///
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    pub(super) fn resolve_draft_flush_target(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, FlushError> {
        self.draft_registry
            .resolve_draft_entity(account_id, draft_key)
            .map_err(|error| {
                FlushError::permanent(format!("draft identity resolution failed: {error}"))
            })
    }

    /// Post-settlement overlay maintenance for a draft save (NS2 Slice 3):
    /// carry the settled draft's visible row across the provider id rotation
    /// until sync confirms it into base.
    ///
    /// A JMAP draft update is create-new + destroy-old, so at settlement the
    /// OLD live row is provider-destroyed (tombstone it if base still shows
    /// it; drop its overlay entry otherwise) and the NEW id has no base row
    /// yet (write the settled fold there — the "applied awaiting convergence"
    /// representation; the post-sync sweep retires it once base covers it).
    /// A newer queued save for the same key refolds through the one lifecycle
    /// function instead.
    pub(super) async fn settle_draft_save_overlay(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        old_live: &str,
        new_live: &str,
        events: &mut Vec<DomainEvent>,
    ) -> Result<(), ServiceError> {
        let new_id = MessageId::from(new_live);
        if old_live != new_live {
            let old_id = MessageId::from(old_live);
            let base_has_old = {
                let overlay = self.overlay.clone();
                let owned_account = account_id.clone();
                let owned_message = old_id.clone();
                offload(move || overlay.read_base_message_record(&owned_account, &owned_message))
                    .await?
                    .is_some()
            };
            {
                let overlay = self.overlay.clone();
                let owned_account = account_id.clone();
                let owned_message = old_id.clone();
                offload(move || {
                    if base_has_old {
                        // Base still shows the destroyed rotation predecessor:
                        // hide it until sync prunes it.
                        overlay.tombstone_overlay_message(&owned_account, &owned_message)
                    } else {
                        overlay.remove_overlay_message(&owned_account, &owned_message)
                    }
                })
                .await?;
            }
            // Prune the stale row client-side.
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                None,
                Some(&old_id),
                serde_json::json!({ "messageId": old_live, "deleted": true }),
            )?);
        }
        // A remaining queued draft op on the same key (a newer save, or a
        // discard racing this settlement) owns the row now — refold it at the
        // new id. Otherwise pin the settled fold there.
        let key = operation.entity.id.as_str();
        let has_remaining_draft_ops = self
            .outbox
            .list_unsettled_operations(account_id)?
            .into_iter()
            .any(|existing| {
                existing.entity.kind == OperationEntityKind::Draft
                    && existing.entity.id == key
                    && matches!(
                        existing.state,
                        OperationState::Pending
                            | OperationState::Inflight
                            | OperationState::Applied
                    )
            });
        if has_remaining_draft_ops {
            self.refresh_message_overlay(
                account_id,
                &new_id,
                crate::service::mutation::OverlayRetire::ConfirmAgainstBase,
            )
            .await?;
        } else if let Ok(posthaste_domain_model::MailIntent::SaveDraft { request, .. }) =
            operation.intent()
        {
            let base = {
                let overlay = self.overlay.clone();
                let owned_account = account_id.clone();
                let owned_message = new_id.clone();
                offload(move || overlay.read_base_message_record(&owned_account, &owned_message))
                    .await?
            };
            let drafts_mailbox = self.drafts_mailbox_id(account_id)?;
            let record = synthesize_draft_record(
                base,
                &request,
                operation,
                drafts_mailbox.as_ref(),
                &new_id,
                key,
            );
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            offload(move || overlay.upsert_overlay_message(&owned_account, &record)).await?;
        }
        // Projection echo at the new id: the client swaps the row instantly
        // instead of waiting for the next sync.
        if let Some(summary) = self
            .message_detail_reader
            .get_message_summary(account_id, &new_id)?
        {
            let scope = summary.mailbox_ids.first().cloned();
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                scope.as_ref(),
                Some(&new_id),
                serde_json::json!({
                    "messageId": new_id.as_str(),
                    "changes": { "mailboxes": true },
                    "projection": &summary,
                }),
            )?);
        }
        Ok(())
    }
}
