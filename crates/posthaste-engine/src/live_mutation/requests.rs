use posthaste_domain_model::{GatewayError, MailboxId, MessageId, SetKeywordsCommand};
use posthaste_provider_call::{CallClass, HttpRequestSpec};
use serde_json::{json, Map, Value};

use crate::live::{map_provider_error, LiveJmapGateway};

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

/// The client-side create id used in the hand-rolled `Mailbox/set` create
/// request. The server echoes it under `created`, keying the created mailbox's
/// server id (see [`crate::live_mutation::outcome::created_mailbox_id`]).
pub(crate) const CREATE_MAILBOX_CREATE_ID: &str = "c0";

/// Build a `Mailbox/set` create request for a new top-level mailbox.
///
/// Flat create — `name` only, no `parentId` (nesting is out of scope).
pub(crate) fn create_mailbox_request_body(account_id: &str, name: &str) -> Value {
    let mut arguments = Map::new();
    arguments.insert(
        "accountId".to_string(),
        Value::String(account_id.to_string()),
    );
    arguments.insert(
        "create".to_string(),
        json!({ CREATE_MAILBOX_CREATE_ID: { "name": name } }),
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

/// Build a `Mailbox/set` destroy request for a single mailbox.
///
/// `onDestroyRemoveEmails` mirrors the confirmed remove-emails flag: when
/// `false` a JMAP server refuses to destroy a non-empty mailbox with a
/// `mailboxHasEmail` set-error (parsed in [`crate::live_mutation::outcome::destroyed_mailbox`]
/// into [`GatewayError::MailboxNotEmpty`]); when `true` the server deletes the
/// contained mail along with the mailbox.
pub(crate) fn destroy_mailbox_request_body(
    account_id: &str,
    mailbox_id: &MailboxId,
    remove_emails: bool,
) -> Value {
    let mut arguments = Map::new();
    arguments.insert(
        "accountId".to_string(),
        Value::String(account_id.to_string()),
    );
    arguments.insert("destroy".to_string(), json!([mailbox_id.as_str()]));
    arguments.insert(
        "onDestroyRemoveEmails".to_string(),
        Value::Bool(remove_emails),
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
    // Serializing our own request is an internal codec fault, not a network
    // error — retrying the wire cannot fix a body we cannot encode.
    let body =
        serde_json::to_vec(&request).map_err(|error| GatewayError::Internal(error.to_string()))?;

    // Route the raw JMAP POST through the provider-call envelope (M31): one
    // shared connection pool (F4), a per-class total deadline (F2), the
    // Retry-After-aware retry loop (F1), and the per-account circuit breaker
    // (D83). This is a metadata-class mutation.
    let bytes = if let Some(executor) = gateway.executor() {
        let spec = HttpRequestSpec::post(
            gateway.client().session().api_url(),
            gateway.client().headers().clone(),
            body,
        );
        executor
            .execute(gateway.account_key(), CallClass::Metadata, spec)
            .await
            .map_err(map_provider_error)?
            .body
    } else {
        // Fallback if the shared client failed to build: the prior direct path.
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
        response
            .bytes()
            .await
            .map_err(|error| GatewayError::Network(error.to_string()))?
            .to_vec()
    };

    // The bytes arrived; a decode failure is an internal/protocol codec fault,
    // not a transient network condition — classify it as such so it is not
    // retried as if the link had dropped.
    serde_json::from_slice(&bytes).map_err(|error| GatewayError::Internal(error.to_string()))
}
