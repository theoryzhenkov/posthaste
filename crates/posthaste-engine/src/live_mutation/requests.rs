use posthaste_domain::{GatewayError, MailboxId, MessageId, SetKeywordsCommand};
use serde_json::{json, Map, Value};

use crate::live::LiveJmapGateway;

/// Build an `Email/set` request for keyword patches.
///
/// JMAP keyword values are presence-only and must be `true`; removing a keyword
/// uses JSON `null` patch syntax rather than `false`.
pub(crate) fn set_keywords_request_body(
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

pub(crate) fn set_mailbox_role_request_body(
    account_id: &str,
    expected_state: Option<&str>,
    mailbox_id: &MailboxId,
    role: Option<&str>,
) -> Value {
    let mut patch = Map::new();
    patch.insert(
        "role".to_string(),
        role.map_or(Value::Null, |role| Value::String(role.to_string())),
    );

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
        json!({ mailbox_id.as_str(): Value::Object(patch) }),
    );

    json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail"
        ],
        "methodCalls": [
            ["Mailbox/set", Value::Object(arguments), "s0"]
        ]
    })
}

pub(crate) async fn send_json_request(
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
