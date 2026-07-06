use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, MutationOutcome, SyncCursor,
    SyncObject,
};

use crate::live::map_gateway_error;

pub(crate) fn set_keywords_mutation_outcome(
    mut response: jmap_client::core::response::EmailSetResponse,
    message_id: &posthaste_domain_model::MessageId,
) -> Result<MutationOutcome, GatewayError> {
    response
        .updated(message_id.as_str())
        .map_err(map_gateway_error)?;
    message_mutation_outcome(response.new_state().to_string())
}

pub(crate) fn mailbox_mutation_outcome(
    mut response: jmap_client::core::response::MailboxSetResponse,
    mailbox_id: &MailboxId,
) -> Result<MutationOutcome, GatewayError> {
    response
        .updated(mailbox_id.as_str())
        .map_err(map_gateway_error)?;
    sync_object_mutation_outcome(SyncObject::Mailbox, response.new_state().to_string())
}

/// Parse the server id of a mailbox created under `create_id` from a
/// `Mailbox/set` response. A `notCreated` entry surfaces as a typed rejection.
pub(crate) fn created_mailbox_id(
    mut response: jmap_client::core::response::MailboxSetResponse,
    create_id: &str,
) -> Result<MailboxId, GatewayError> {
    let created = response.created(create_id).map_err(map_gateway_error)?;
    created
        .id()
        .map(MailboxId::from)
        .ok_or_else(|| GatewayError::Rejected("Mailbox/set create returned no id".to_string()))
}

/// Parse a `Mailbox/set` destroy response for a single mailbox.
///
/// A `destroyed` entry is success. A `notDestroyed` `mailboxHasEmail` (the
/// server refusing an `onDestroyRemoveEmails=false` destroy of a non-empty
/// mailbox) is surfaced as the typed [`GatewayError::MailboxNotEmpty`] backstop
/// — the count is not in the JMAP response (the service gate carries the real
/// count), so `0` stands for "unknown, at least one". Any other `notDestroyed`
/// set-error maps through the ordinary gateway-error path.
pub(crate) fn destroyed_mailbox(
    mut response: jmap_client::core::response::MailboxSetResponse,
    mailbox_id: &MailboxId,
) -> Result<(), GatewayError> {
    match response.destroyed(mailbox_id.as_str()) {
        Ok(()) => Ok(()),
        Err(jmap_client::Error::Set(error))
            if matches!(
                error.error(),
                jmap_client::core::set::SetErrorType::MailboxHasEmail
            ) =>
        {
            Err(GatewayError::MailboxNotEmpty { count: 0 })
        }
        Err(error) => Err(map_gateway_error(error)),
    }
}

/// Build a `MutationOutcome` with a message-type sync cursor from the server's new state string.
///
/// @spec docs/L1-jmap#core-types
/// @spec docs/L1-sync#state-management
pub(crate) fn message_mutation_outcome(state: String) -> Result<MutationOutcome, GatewayError> {
    sync_object_mutation_outcome(
        SyncObject::Message,
        crate::sync::encode_email_cursor_state(&state),
    )
}

fn sync_object_mutation_outcome(
    object_type: SyncObject,
    state: String,
) -> Result<MutationOutcome, GatewayError> {
    Ok(MutationOutcome {
        cursor: Some(SyncCursor {
            object_type,
            state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        }),
        message: None,
    })
}
