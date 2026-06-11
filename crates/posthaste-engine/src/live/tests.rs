use super::*;

#[test]
fn jmap_credentials_use_bearer_without_username() {
    assert_eq!(
        jmap_credentials(None, "token"),
        Credentials::bearer("token")
    );
    assert_eq!(
        jmap_credentials(Some("  "), "token"),
        Credentials::bearer("token")
    );
}

#[test]
fn jmap_credentials_use_basic_with_username() {
    assert_eq!(
        jmap_credentials(Some("alice@example.com"), "secret"),
        Credentials::basic("alice@example.com", "secret")
    );
}

#[test]
fn missing_method_response_becomes_gateway_rejected_error() {
    let error = required_method_response::<()>(None, "Email/get")
        .expect_err("missing responses should be rejected");

    match error {
        GatewayError::Rejected(message) => {
            assert_eq!(message, "Email/get response missing");
        }
        other => panic!("expected rejected error, got {other:?}"),
    }
}

#[test]
fn set_errors_become_gateway_rejected_errors() {
    let set_error: jmap_client::core::set::SetError<String> =
        serde_json::from_value(serde_json::json!({
            "type": "noRecipients",
            "description": "No recipients found in email."
        }))
        .expect("set error should deserialize");

    let error = map_gateway_error(set_error.into());

    match error {
        GatewayError::Rejected(message) => {
            assert_eq!(message, "noRecipients: No recipients found in email.");
        }
        other => panic!("expected rejected error, got {other:?}"),
    }
}
