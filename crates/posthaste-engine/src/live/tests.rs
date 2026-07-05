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

// ---------------------------------------------------------------------------
// Send-phase dispatch classification (the duplicate-send fix, DP-C5/C6).
//
// `classify_send_dispatch_error` must split by PHASE, not error type: a
// transport failure that is PROVABLY pre-write (connect refused) is a safe
// retryable `Network`, while a transport failure at/after the request write
// (a lost response) is `DispatchUncertain` so the outbox parks it and never
// blind-resends. These build REAL `reqwest::Error`s against localhost so the
// `is_connect()` phase discriminator is exercised, not mocked.
// ---------------------------------------------------------------------------

/// A real connect-phase `reqwest::Error`: nothing listens on port 1, so the
/// TCP connect is refused before any request byte is written.
async fn connect_refused_transport_error() -> jmap_client::Error {
    let error = reqwest::Client::new()
        .get("http://127.0.0.1:1/")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .expect_err("connect to port 1 must fail");
    assert!(error.is_connect(), "expected a connect-phase error");
    jmap_client::Error::Transport(error)
}

/// A real post-connect `reqwest::Error`: the server accepts the TCP connection
/// (so the request bytes are written) then drops it before responding — the
/// EmailSubmission's fate is unknown.
async fn response_lost_transport_error() -> jmap_client::Error {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        // Accept one connection, read a little of the request, then drop the
        // socket without writing any response.
        if let Ok((stream, _)) = listener.accept().await {
            drop(stream);
        }
    });
    let error = reqwest::Client::new()
        .get(format!("http://{addr}/"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .expect_err("dropped connection must fail the request");
    assert!(
        !error.is_connect(),
        "the connection was established, so this is not a connect-phase error"
    );
    jmap_client::Error::Transport(error)
}

#[tokio::test]
async fn send_pre_write_connect_failure_is_retryable_network() {
    // A genuinely offline / connection-refused send is provably pre-write: safe
    // to auto-retry, so it stays `Network` (→ Transient in the outbox).
    let classified = classify_send_dispatch_error(connect_refused_transport_error().await);
    assert!(
        matches!(classified, GatewayError::Network(_)),
        "connect-refused must classify Network (retryable), got {classified:?}"
    );
}

#[tokio::test]
async fn send_lost_response_after_write_is_dispatch_uncertain() {
    // The request bytes were written and the response was lost: the submission
    // may already have committed, so it must be dispatch-uncertain (park, never
    // blind-resend) — NOT a blind-retryable Network error.
    let classified = classify_send_dispatch_error(response_lost_transport_error().await);
    assert!(
        matches!(classified, GatewayError::DispatchUncertain(_)),
        "a lost response after the write must be DispatchUncertain, got {classified:?}"
    );
}

#[test]
fn send_server_answer_keeps_ordinary_classification() {
    // A structured server response means the server ANSWERED the send request:
    // the outcome is determined, not unknown, so it keeps its ordinary mapping
    // (here a permanent 4xx → Rejected) rather than being parked uncertain.
    let classified = classify_send_dispatch_error(jmap_client::Error::Server(
        "404 Not Found".to_string(),
    ));
    assert!(
        matches!(classified, GatewayError::Rejected(_)),
        "a server answer is a known outcome, got {classified:?}"
    );

    // A non-transport error is likewise not a lost-response condition, so it
    // keeps its ordinary mapping rather than being parked uncertain.
    let internal = classify_send_dispatch_error(jmap_client::Error::Internal(
        "decode".to_string(),
    ));
    assert!(
        matches!(internal, GatewayError::Network(_)),
        "a non-transport internal error keeps its ordinary mapping, got {internal:?}"
    );
}
