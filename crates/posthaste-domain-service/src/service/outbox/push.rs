//! Provider dispatch: push one operation to the gateway per
//! [`OperationKind`], mapping the result to a [`Pushed`] outcome or a typed
//! flush error.

use super::classify::{classify_gateway_error, FlushError};
use crate::service::*;
use posthaste_domain_model::{MessageReadback, MutationOutcome};

/// Result of pushing one operation to the provider.
#[allow(clippy::large_enum_variant)]
pub(super) enum Pushed {
    /// A non-message entity op (draft/send): settle and remove.
    /// `assigned_entity_id` is the provider id a draft save returned — the
    /// settlement repoints the op's stable draft key to it in the registry
    /// (M70/D136: the op's entity id IS the key and never rotates).
    /// `destroyed_entity_id` is the live id a draft destroy resolved to at
    /// flush time, so the settlement's reconciling event names the projected
    /// row rather than the stable key the op carries.
    Entity {
        assigned_entity_id: Option<String>,
        destroyed_entity_id: Option<String>,
    },
    /// A message state assertion: settle now via the provider readback.
    /// `rejected` is `Some(reason)` when the provider rejected the change — the
    /// readback then carries the unchanged state, so the settle write reverts.
    Message {
        readback: Option<MessageReadback>,
        rejected: Option<String>,
    },
}

/// Normalize a message-mutation gateway result into a [`Pushed::Message`]:
/// `Ok` (accepted) and `MutationRejected` (rejected) both carry a readback and
/// settle in one path; only a transport error is a flush error (retry).
fn message_pushed(result: Result<MutationOutcome, GatewayError>) -> Result<Pushed, FlushError> {
    match result {
        Ok(outcome) => Ok(Pushed::Message {
            readback: outcome.message,
            rejected: None,
        }),
        Err(GatewayError::MutationRejected { readback, reason }) => Ok(Pushed::Message {
            readback: Some(*readback),
            rejected: Some(reason),
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
        match operation.kind {
            OperationKind::DraftCreate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
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
                })
            }
            OperationKind::DraftUpdate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                // M70 (D136): the op carries the stable draft key; resolve it
                // to the CURRENT live id here, immediately before the gateway
                // call, so the replace targets the freshest mapping (in-session
                // rotations and sync-observed ones alike).
                let replace_id =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?;
                let replace = MessageId::from(replace_id.as_str());
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
                        Some(&replace),
                        idempotent_redelivery,
                        operation.id.as_str(),
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                    destroyed_entity_id: None,
                })
            }
            OperationKind::DraftDelete => {
                // M70 (D136): resolve the stable key to the live destroy target
                // at flush (see [`Self::resolve_draft_flush_target`]) — a
                // registry repoint between enqueue and flush retargets the
                // destroy to the draft's current live id.
                let target_id =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?;
                let target = MessageId::from(target_id.as_str());
                // D133: only an idempotent redelivery (a send-consume re-enqueue)
                // masks a provider `notFound` as success; a user discard's
                // `notFound` surfaces as a retryable failure.
                let idempotent_redelivery = operation
                    .payload
                    .get("idempotentRedelivery")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                gateway
                    .delete_draft(account_id, &target, idempotent_redelivery)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: Some(target_id),
                })
            }
            OperationKind::Send => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                // The operation id is the send's stable idempotency identity
                // (constant across retries): the gateway derives the JMAP
                // EmailSubmission create-id + `ifInState` and the SMTP/JMAP
                // Message-ID from it (D84/D85), so a re-forward of a send that
                // already committed is deduplicated, not duplicated.
                gateway
                    .send_message(account_id, &request, operation.id.as_str())
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: None,
                })
            }
            OperationKind::SetKeywords => {
                let command = parse_payload::<SetKeywordsCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(
                    gateway
                        .set_keywords(account_id, &target, None, &command)
                        .await,
                )
            }
            OperationKind::ReplaceMailboxes => {
                let command = parse_payload::<ReplaceMailboxesCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(
                    gateway
                        .replace_mailboxes(account_id, &target, None, &command.mailbox_ids)
                        .await,
                )
            }
            OperationKind::Destroy => {
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(gateway.destroy_message(account_id, &target, None).await)
            }
        }
    }
}

fn parse_payload<T: serde::de::DeserializeOwned>(operation: &Operation) -> Result<T, FlushError> {
    serde_json::from_value(operation.payload.clone()).map_err(|error| {
        FlushError::permanent(format!("invalid {:?} payload: {error}", operation.kind))
    })
}
