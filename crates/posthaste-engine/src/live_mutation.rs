mod outcome;
mod requests;

use posthaste_domain::{
    GatewayError, MailboxId, MailboxRole, MessageId, MutationOutcome, SetKeywordsCommand,
};

use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};
use crate::live_mutation::outcome::{
    mailbox_mutation_outcome, message_mutation_outcome, set_keywords_mutation_outcome,
};
use crate::live_mutation::requests::{
    send_json_request, set_keywords_request_body, set_mailbox_role_request_body,
};

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
    let request = set_keywords_request_body(
        gateway.server_account_id(),
        expected_state,
        message_id,
        command,
    );
    let mut response = send_json_request(gateway, request).await?;
    let response = required_method_response(response.pop_method_response(), "Email/set")?
        .unwrap_set_email()
        .map_err(map_gateway_error)?;
    set_keywords_mutation_outcome(response, message_id)
}

/// Update a mailbox role via `Mailbox/set`.
///
/// Uses raw JSON requests so clearing a role can send an explicit JSON `null`.
/// When another mailbox already owns the requested role, clears that mailbox
/// first and then assigns the role using the returned mailbox state. This
/// ordering matches servers that validate role uniqueness during each update.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#conflict-model
pub(crate) async fn set_mailbox_role(
    gateway: &LiveJmapGateway,
    mailbox_id: &MailboxId,
    expected_state: Option<&str>,
    role: Option<&str>,
    clear_role_from: Option<&MailboxId>,
) -> Result<MutationOutcome, GatewayError> {
    validate_mailbox_role(role)?;
    let mut assignment_expected_state = expected_state.map(str::to_string);
    if let Some(clear_role_from) = clear_role_from.filter(|id| *id != mailbox_id) {
        let request = set_mailbox_role_request_body(
            gateway.server_account_id(),
            assignment_expected_state.as_deref(),
            clear_role_from,
            None,
        );
        let mut response = send_json_request(gateway, request).await?;
        let response = required_method_response(response.pop_method_response(), "Mailbox/set")?
            .unwrap_set_mailbox()
            .map_err(map_gateway_error)?;
        let outcome = mailbox_mutation_outcome(response, clear_role_from)?;
        assignment_expected_state = outcome.cursor.map(|cursor| cursor.state);
    }

    let request = set_mailbox_role_request_body(
        gateway.server_account_id(),
        assignment_expected_state.as_deref(),
        mailbox_id,
        role,
    );
    let mut response = send_json_request(gateway, request).await?;
    let response = required_method_response(response.pop_method_response(), "Mailbox/set")?
        .unwrap_set_mailbox()
        .map_err(map_gateway_error)?;
    mailbox_mutation_outcome(response, mailbox_id)
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

fn validate_mailbox_role(role: Option<&str>) -> Result<(), GatewayError> {
    match role {
        None => Ok(()),
        Some(value) if MailboxRole::parse(value).is_some() => Ok(()),
        Some(other) => Err(GatewayError::Rejected(format!(
            "unsupported mailbox role: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests;
