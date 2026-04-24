use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, MessageId, MutationOutcome,
    SetKeywordsCommand, SyncCursor, SyncObject,
};

use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};

/// Add or remove keywords (flags) on a message via `Email/set`.
///
/// Uses `ifInState` for optimistic concurrency when `expected_state` is provided.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#conflict-model
pub(crate) async fn set_keywords(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
    expected_state: Option<&str>,
    command: &SetKeywordsCommand,
) -> Result<MutationOutcome, GatewayError> {
    let mut request = gateway.client().build();
    let set = request.set_email();
    if let Some(expected_state) = expected_state {
        set.if_in_state(expected_state);
    }
    let update = set.update(message_id.as_str());
    for keyword in &command.add {
        update.keyword(keyword.as_str(), true);
    }
    for keyword in &command.remove {
        update.keyword(keyword.as_str(), false);
    }
    let mut response = gateway.send_request(request).await?;
    let response = required_method_response(response.pop_method_response(), "Email/set")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    message_mutation_outcome(response.new_state().to_string())
}

/// Replace a message's mailbox membership via `Email/set`.
///
/// Used for move and archive operations. Supports optimistic concurrency.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#conflict-model
pub(crate) async fn replace_mailboxes(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
    expected_state: Option<&str>,
    mailbox_ids: &[MailboxId],
) -> Result<MutationOutcome, GatewayError> {
    let mut request = gateway.client().build();
    let set = request.set_email();
    if let Some(expected_state) = expected_state {
        set.if_in_state(expected_state);
    }
    set.update(message_id.as_str())
        .mailbox_ids(mailbox_ids.iter().map(MailboxId::as_str));
    let mut response = gateway.send_request(request).await?;
    let response = required_method_response(response.pop_method_response(), "Email/set")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    message_mutation_outcome(response.new_state().to_string())
}

/// Permanently destroy a message via `Email/set`.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#conflict-model
pub(crate) async fn destroy_message(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
    expected_state: Option<&str>,
) -> Result<MutationOutcome, GatewayError> {
    let mut request = gateway.client().build();
    let set = request.set_email();
    if let Some(expected_state) = expected_state {
        set.if_in_state(expected_state);
    }
    set.destroy([message_id.as_str()]);
    let mut response = gateway.send_request(request).await?;
    let response = required_method_response(response.pop_method_response(), "Email/set")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    message_mutation_outcome(response.new_state().to_string())
}

/// Build a `MutationOutcome` with a message-type sync cursor from the server's new state string.
///
/// @spec docs/L1-jmap#core-types
/// @spec docs/L1-sync#state-management
fn message_mutation_outcome(state: String) -> Result<MutationOutcome, GatewayError> {
    Ok(MutationOutcome {
        cursor: Some(SyncCursor {
            object_type: SyncObject::Message,
            state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_mutation_outcome_wraps_message_cursor() {
        let outcome =
            message_mutation_outcome("message-9".to_string()).expect("cursor should build");
        let cursor = outcome.cursor.expect("cursor should be present");
        assert_eq!(cursor.object_type, SyncObject::Message);
        assert_eq!(cursor.state, "message-9");
        assert!(!cursor.updated_at.is_empty());
    }
}
