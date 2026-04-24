use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, MessageId, MutationOutcome,
    SetKeywordsCommand, SyncCursor, SyncObject,
};
use serde_json::{json, Map, Value};

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

/// Build an `Email/set` request for keyword patches.
///
/// JMAP keyword values are presence-only and must be `true`; removing a keyword
/// uses JSON `null` patch syntax rather than `false`.
fn set_keywords_request_body(
    account_id: &str,
    expected_state: Option<&str>,
    message_id: &MessageId,
    command: &SetKeywordsCommand,
) -> Value {
    let mut patch = Map::new();
    for keyword in &command.add {
        patch.insert(format!("keywords/{keyword}"), Value::Bool(true));
    }
    for keyword in &command.remove {
        patch.insert(format!("keywords/{keyword}"), Value::Null);
    }

    let mut arguments = Map::new();
    arguments.insert(
        "accountId".to_string(),
        Value::String(account_id.to_string()),
    );
    if let Some(expected_state) = expected_state {
        arguments.insert(
            "ifInState".to_string(),
            Value::String(expected_state.to_string()),
        );
    }
    arguments.insert(
        "update".to_string(),
        json!({ message_id.as_str(): Value::Object(patch) }),
    );

    json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail"
        ],
        "methodCalls": [
            ["Email/set", Value::Object(arguments), "s0"]
        ]
    })
}

async fn send_json_request(
    gateway: &LiveJmapGateway,
    request: Value,
) -> Result<
    jmap_client::core::response::Response<jmap_client::core::response::TaggedMethodResponse>,
    GatewayError,
> {
    let body = serde_json::to_string(&request)
        .map_err(|error| GatewayError::Network(error.to_string()))?;
    let response = reqwest::Client::builder()
        .timeout(gateway.client().timeout())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| GatewayError::Network(error.to_string()))?
        .post(gateway.client().session().api_url())
        .headers(gateway.client().headers().clone())
        .body(body)
        .send()
        .await
        .map_err(|error| GatewayError::Network(error.to_string()))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GatewayError::Auth);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(GatewayError::Network(format!(
            "JMAP request failed with HTTP {status}: {body}"
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| GatewayError::Network(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| GatewayError::Network(error.to_string()))
}

fn set_keywords_mutation_outcome(
    mut response: jmap_client::core::response::EmailSetResponse,
    message_id: &MessageId,
) -> Result<MutationOutcome, GatewayError> {
    response
        .updated(message_id.as_str())
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
    fn set_keywords_request_uses_null_to_remove_seen() {
        let request = set_keywords_request_body(
            "account-1",
            Some("state-1"),
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: vec!["$seen".to_string()],
            },
        );

        assert_eq!(
            request["methodCalls"][0][1]["update"]["message-1"]["keywords/$flagged"],
            Value::Bool(true)
        );
        assert_eq!(
            request["methodCalls"][0][1]["update"]["message-1"]["keywords/$seen"],
            Value::Null
        );
        assert_eq!(request["methodCalls"][0][1]["ifInState"], "state-1");
    }

    #[test]
    fn set_keywords_outcome_requires_target_id_to_be_updated() {
        let response: jmap_client::core::response::EmailSetResponse =
            serde_json::from_value(json!({
                "accountId": "account-1",
                "oldState": "state-1",
                "newState": "state-2",
                "notUpdated": {
                    "message-1": {
                        "type": "invalidProperties",
                        "description": "bad keyword patch"
                    }
                }
            }))
            .expect("set response should deserialize");

        let error = set_keywords_mutation_outcome(response, &MessageId::from("message-1"))
            .expect_err("notUpdated must not be treated as success");

        match error {
            GatewayError::Rejected(message) => {
                assert!(message.contains("bad keyword patch"));
            }
            other => panic!("expected rejected error, got {other:?}"),
        }
    }

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
