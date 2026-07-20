//! Provider dispatch: push one operation to the gateway per
//! [`OperationKind`], mapping the result to a [`Pushed`] outcome or a typed
//! flush error.

use super::classify::{classify_gateway_error, FlushDisposition, FlushError};
use crate::service::*;
use posthaste_domain_model::MailIntent;
use posthaste_domain_model::{MessageReadback, MutationOutcome};

/// Result of pushing one operation to the provider.
#[allow(clippy::large_enum_variant)]
pub(super) enum Pushed {
    /// A non-message entity op (draft/send).
    /// `assigned_entity_id` is the provider id a draft save returned — the
    /// settlement repoints the op's stable draft key to it in the registry (the
    /// op's entity id IS the key and never rotates), and it is the adoption
    /// bridge for a JMAP draft: the provider now holds the copy at this id.
    /// `destroyed_entity_id` is the live id a draft destroy resolved to at
    /// flush time, so the settlement's reconciling event names the projected
    /// row rather than the stable key the op carries.
    /// `cursor` is the provider sync position a save/send returned, when the
    /// gateway exposes one: a content op that rests settled (`applied`) records
    /// it as its causal-truncation watermark, exactly like a blind-settled
    /// message assertion. `None` falls back to the cycle rule (IMAP; a gateway
    /// that returns no state).
    Entity {
        assigned_entity_id: Option<String>,
        destroyed_entity_id: Option<String>,
        /// For an applied send: the typed Sent-copy filing outcome.
        send_filing: Option<posthaste_domain_model::SendFiling>,
        cursor: Option<posthaste_domain_model::SyncCursor>,
    },
    /// A message state assertion: settle now via the provider readback.
    /// `rejected` is `Some(reason)` when the provider rejected the change — the
    /// readback then carries the unchanged state, so the settle write reverts.
    /// `cursor` is the provider sync position the mutation returned (a JMAP
    /// `set` `newState`): a blind settlement (no readback) records it as the
    /// op's causal-truncation watermark. `retargeted_to` is `Some(adopted_id)`
    /// when the assertion was retargeted from a provisional `send-<id>` to its
    /// adopted real id — the settlement then writes base for the adopted id
    /// AND re-derives the op's `send-<id>` (the pre-adoption fold reverts).
    /// `None` for a normal assertion (the op's entity id IS the target).
    Message {
        readback: Option<MessageReadback>,
        rejected: Option<String>,
        cursor: Option<posthaste_domain_model::SyncCursor>,
        retargeted_to: Option<MessageId>,
    },
    /// A state-assertion op against a provisional `send-<id>` whose send was
    /// NOT adopted (failed or gone): there is no real message to assert on, so
    /// the op settles as applied and leaves the log. The overlay re-derives
    /// the `send-<id>` (a tombstone reverts; the send op's own fold, if any,
    /// reproduces the row). No base write — the provider holds nothing to
    /// assert against.
    NoOp,
}

/// Normalize a message-mutation gateway result into a [`Pushed::Message`]:
/// `Ok` (accepted) and `MutationRejected` (rejected) both carry a readback and
/// settle in one path; only a transport error is a flush error (retry).
/// `retargeted_to` is `Some` when the assertion was retargeted from a
/// provisional `send-<id>` to its adopted real id (see [`Pushed::Message`]).
fn message_pushed(
    result: Result<MutationOutcome, GatewayError>,
    retargeted_to: Option<MessageId>,
) -> Result<Pushed, FlushError> {
    match result {
        Ok(outcome) => Ok(Pushed::Message {
            readback: outcome.message,
            rejected: None,
            cursor: outcome.cursor,
            retargeted_to,
        }),
        Err(GatewayError::MutationRejected { readback, reason }) => Ok(Pushed::Message {
            readback: Some(*readback),
            rejected: Some(reason),
            cursor: None,
            retargeted_to,
        }),
        Err(transport) => Err(classify_gateway_error(transport)),
    }
}

impl MailService {
    /// Push a single operation to the provider, mapping the result to a
    /// settlement or a typed flush error.
    pub(super) async fn push_operation(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        gateway: &dyn MailGateway,
    ) -> Result<Pushed, FlushError> {
        // NS2 Slice 2: ONE decode boundary — the typed intent — instead of a
        // per-arm parse_payload with per-site fallbacks.
        let intent = operation.intent().map_err(FlushError::permanent)?;
        match intent {
            MailIntent::SaveDraft {
                create: true,
                request,
            } => {
                // A create has no replace target, so the DS3 redelivery flag is
                // irrelevant (no destroy outcome to mask). The operation id is the
                // stable create identity (constant across retries): the gateway
                // derives a deterministic `Email/set` create-id from it (DS2), so a
                // lost-response redelivery re-creates under the same id and cannot
                // orphan a twin draft.
                let new_id = gateway
                    .save_draft(account_id, &request, None, false, operation.id.as_str())
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                    destroyed_entity_id: None,
                    send_filing: None,
                    cursor: None,
                })
            }
            MailIntent::SaveDraft {
                create: false,
                request,
            } => {
                // M70 (D136): the op carries the stable draft key; resolve it
                // to the CURRENT live id here, immediately before the gateway
                // call, so the replace targets the freshest mapping (in-session
                // rotations and sync-observed ones alike). A typed miss (D153)
                // means the draft was CONFIRMED destroyed since enqueue (e.g.
                // deleted on another device) — the queued edit still wants
                // saving, so push it as a fresh create (last-writer-wins; the
                // settlement re-registers the mapping).
                let replace_id =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?;
                let replace = replace_id.as_deref().map(MessageId::from);
                // DS3/D133: a re-flush of this save (attempts > 0) may have already
                // committed the prior-draft destroy on an earlier attempt, so an
                // already-gone replace target is benign; a first delivery's failed
                // replace-destroy surfaces so the save is retried rather than
                // silently leaving the old draft behind (the twin).
                let idempotent_redelivery = operation.attempts > 0;
                // The operation id is the stable create identity (constant across
                // retries): the gateway derives a deterministic `Email/set`
                // create-id from it (DS2), so a redelivery whose create+destroy
                // committed but whose response was lost re-creates under the same
                // id and cannot orphan a twin draft.
                let new_id = gateway
                    .save_draft(
                        account_id,
                        &request,
                        replace.as_ref(),
                        idempotent_redelivery,
                        operation.id.as_str(),
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                    destroyed_entity_id: None,
                    send_filing: None,
                    cursor: None,
                })
            }
            MailIntent::DiscardDraft {
                idempotent_redelivery,
            } => {
                // M70 (D136): resolve the stable key to the live destroy target
                // at flush (see [`Self::resolve_draft_flush_target`]) — a
                // registry repoint between enqueue and flush retargets the
                // destroy to the draft's current live id. A typed miss (D153):
                // the registry forgets only on CONFIRMED destruction, so the
                // draft is already gone — settle as done without a provider
                // call.
                let Some(target_id) =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?
                else {
                    return Ok(Pushed::Entity {
                        assigned_entity_id: None,
                        destroyed_entity_id: Some(operation.entity.id.clone()),
                        send_filing: None,
                        cursor: None,
                    });
                };
                let target = MessageId::from(target_id.as_str());
                // D133: only an idempotent redelivery (a send-consume re-enqueue)
                // masks a provider `notFound` as success; a user discard's
                // `notFound` surfaces as a retryable failure.
                gateway
                    .delete_draft(account_id, &target, idempotent_redelivery)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: Some(target_id),
                    send_filing: None,
                    cursor: None,
                })
            }
            MailIntent::Send(request) => {
                // The operation id is the send's stable idempotency identity
                // (constant across retries): the gateway derives the JMAP
                // EmailSubmission create-id + `ifInState` and the SMTP/JMAP
                // Message-ID from it (D84/D85), so a re-forward of a send that
                // already committed is deduplicated, not duplicated.
                // Gateway-owned consumption (NS2 Slice 4): the send's
                // materialized originating-draft key (stamped at admission,
                // D170) resolves to its LIVE provider id here at flush — a
                // typed miss means the draft was confirmed destroyed since
                // admission, so there is nothing to consume.
                let consume = match request
                    .draft_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                {
                    Some(key) => self
                        .resolve_draft_flush_target(account_id, key)?
                        .map(|live| MessageId::from(live.as_str())),
                    None => None,
                };
                let filing = gateway
                    .send_message(
                        account_id,
                        &request,
                        consume.as_ref(),
                        operation.id.as_str(),
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: None,
                    send_filing: Some(filing),
                    cursor: None,
                })
            }
            MailIntent::SetKeywords(command) => {
                match self.resolve_assertion_target(account_id, operation)? {
                    Some((target, retargeted_to)) => message_pushed(
                        gateway
                            .set_keywords(account_id, &target, None, &command)
                            .await,
                        retargeted_to,
                    ),
                    None => Ok(Pushed::NoOp),
                }
            }
            MailIntent::ReplaceMailboxes(command) => {
                match self.resolve_assertion_target(account_id, operation)? {
                    Some((target, retargeted_to)) => message_pushed(
                        gateway
                            .replace_mailboxes(account_id, &target, None, &command.mailbox_ids)
                            .await,
                        retargeted_to,
                    ),
                    None => Ok(Pushed::NoOp),
                }
            }
            MailIntent::Destroy => match self.resolve_assertion_target(account_id, operation)? {
                Some((target, retargeted_to)) => message_pushed(
                    gateway.destroy_message(account_id, &target, None).await,
                    retargeted_to,
                ),
                None => Ok(Pushed::NoOp),
            },
        }
    }

    /// Resolve a state-assertion op's (Destroy/ReplaceMailboxes/SetKeywords)
    /// target id. For a normal message id the target is the op's entity id.
    /// For a provisional `send-<id>` (a not-yet-adopted sent message — no IMAP
    /// message behind it), resolve the adoption alias:
    /// - alias set → retarget to the adopted real id (the op asserts on it);
    ///   `retargeted_to` carries the adopted id so the settlement writes base
    ///   for it AND re-derives the op's `send-<id>` (the pre-adoption fold
    ///   reverts).
    /// - alias absent + send op still in flight (pending/inflight/applied) →
    ///   `Err(Defer)`: re-queue without bumping attempts; the next flush after
    ///   adoption retargets.
    /// - alias absent + send op terminal (failed/dispatchUncertain) or gone →
    ///   `Ok(None)`: no-op (nothing to assert; the send produced no real copy
    ///   or was discarded).
    ///
    /// Store errors map to `Permanent` (matching `resolve_draft_flush_target`).
    fn resolve_assertion_target(
        &self,
        account_id: &AccountId,
        operation: &Operation,
    ) -> Result<Option<(MessageId, Option<MessageId>)>, FlushError> {
        let entity_id = operation.entity.id.as_str();
        if !posthaste_domain_model::is_provisional_sent_id(entity_id) {
            return Ok(Some((MessageId::from(entity_id), None)));
        }
        if let Some(adopted) = self
            .send_registry
            .resolve_send_alias(account_id, entity_id)
            .map_err(|error| {
                FlushError::permanent(format!("send alias resolution failed: {error}"))
            })?
        {
            return Ok(Some((
                MessageId::from(adopted.as_str()),
                Some(MessageId::from(entity_id)),
            )));
        }
        // Alias absent: the send isn't adopted. Find the send op to decide
        // whether to defer (in flight) or no-op (terminal/gone). At most one
        // send op exists per `send-<id>`.
        let send_op = self
            .outbox
            .find_operation_by_entity_id(account_id, entity_id, OperationKind::Send)
            .map_err(|error| FlushError::permanent(format!("send op lookup failed: {error}")))?;
        match send_op {
            Some(op) if !op.state.is_terminal() => Err(FlushError {
                disposition: FlushDisposition::Defer,
                message:
                    "state assertion on a provisional send-<id> deferred: send not yet adopted"
                        .to_string(),
            }),
            Some(_) | None => Ok(None),
        }
    }
}
