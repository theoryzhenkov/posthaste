//! HTTP-level tests for the API surface: a real backend assembled over
//! temporary directories, served on an ephemeral loopback port, and driven
//! through plain HTTP — the same way every client consumes it.

use std::sync::Arc;
use std::time::Duration;

use posthaste_client_backend::{serve, AppPaths, AppState, BuildOptions, ServerHandle};
use posthaste_domain_model::{SecretRef, SecretStoreError};
use posthaste_domain_service::SecretStore;

/// Keychain-free secret store: no account in these tests resolves a secret.
struct TestSecretStore;

impl SecretStore for TestSecretStore {
    fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        Err(SecretStoreError::Unavailable("test".to_string()))
    }

    fn save(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }

    fn update(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }

    fn delete(&self, _secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

struct TestServer {
    state: AppState,
    server: ServerHandle,
    token: String,
    http: reqwest::Client,
    /// Owns the config/state roots for the server's lifetime.
    _dir: tempfile::TempDir,
}

impl TestServer {
    async fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));
        let mut options = BuildOptions::at(paths);
        options.secret_store = Some(Arc::new(TestSecretStore));
        let state = AppState::assemble(options).await.expect("assemble backend");
        let token = "test-session-token".to_string();
        let server = serve(state.clone(), 0, token.clone())
            .await
            .expect("bind ephemeral port");
        Self {
            state,
            server,
            token,
            http: reqwest::Client::new(),
            _dir: dir,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.server.addr)
    }

    async fn post_json(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.http
            .post(self.url(path))
            .header("authorization", format!("Bearer {}", self.token))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("request completes")
    }

    async fn shutdown(self) {
        self.server.abort();
        self.state.shutdown().await;
    }
}

async fn json_body(response: reqwest::Response) -> serde_json::Value {
    let text = response.text().await.expect("read body");
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("invalid JSON ({error}): {text}"))
}

/// Reads SSE `data:` payloads off a streaming response.
struct SseReader {
    response: reqwest::Response,
    buffer: String,
}

impl SseReader {
    async fn connect(server: &TestServer) -> Self {
        let response = server
            .http
            .get(server.url(&format!("/events?token={}", server.token)))
            .send()
            .await
            .expect("connect event stream");
        assert_eq!(response.status(), 200);
        Self {
            response,
            buffer: String::new(),
        }
    }

    /// The next `data:` payload, waiting up to `deadline`.
    async fn next_data(&mut self, deadline: Duration) -> serde_json::Value {
        tokio::time::timeout(deadline, async {
            loop {
                if let Some(position) = self.buffer.find("\n\n") {
                    let frame = self.buffer[..position].to_string();
                    self.buffer.drain(..position + 2);
                    if let Some(data) = frame.lines().find_map(|line| {
                        line.strip_prefix("data: ")
                            .or_else(|| line.strip_prefix("data:"))
                    }) {
                        return serde_json::from_str(data).expect("SSE data is JSON");
                    }
                    continue;
                }
                let chunk = self
                    .response
                    .chunk()
                    .await
                    .expect("stream read")
                    .expect("stream stays open");
                self.buffer.push_str(&String::from_utf8_lossy(&chunk));
            }
        })
        .await
        .expect("SSE message within the deadline")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_the_session_token_are_rejected() {
    let server = TestServer::spawn().await;

    // No credentials at all.
    let response = server
        .http
        .post(server.url("/api/query"))
        .body(serde_json::json!({ "accounts": {} }).to_string())
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 401);
    let body = json_body(response).await;
    assert_eq!(body["kind"], "unauthorized");
    assert_eq!(body["retryable"], false);

    // A wrong bearer token.
    let response = server
        .http
        .post(server.url("/query"))
        .header("authorization", "Bearer not-the-token")
        .body(serde_json::json!({ "accounts": {} }).to_string())
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 401);

    // The event stream rejects a wrong query token.
    let response = server
        .http
        .get(server.url("/events?token=wrong"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 401);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn query_round_trip_stamps_the_generation_observed_before_evaluation() {
    let server = TestServer::spawn().await;

    let response = server
        .post_json("/api/query", serde_json::json!({ "accounts": {} }))
        .await;
    assert_eq!(response.status(), 200);
    let body = json_body(response).await;
    let first_generation = body["generation"].as_u64().expect("generation stamp");
    assert_eq!(body["data"]["rows"], serde_json::json!([]));

    // A write moves the generation; the next answer is stamped at or past it.
    let response = server
        .post_json(
            "/api/command",
            serde_json::json!({
                "id": "cmd-account-1",
                "command": { "createAccount": { "name": "Ada" } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let accepted = json_body(response).await;
    let write_generation = accepted["generation"].as_u64().expect("generation");
    assert!(write_generation > first_generation);

    let response = server
        .post_json("/query", serde_json::json!({ "accounts": {} }))
        .await;
    let body = json_body(response).await;
    assert!(body["generation"].as_u64().expect("generation") >= write_generation);
    let rows = body["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Ada");

    // A malformed query is the models error envelope, not a bare failure.
    let response = server
        .post_json("/api/query", serde_json::json!({ "nonsense": {} }))
        .await;
    assert_eq!(response.status(), 400);
    let body = json_body(response).await;
    assert_eq!(body["kind"], "malformedRequest");

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replaying_a_command_id_returns_the_original_outcome_without_reapplying() {
    let server = TestServer::spawn().await;

    let envelope = serde_json::json!({
        "id": "cmd-idempotent-1",
        "command": { "createAccount": { "name": "Grace" } }
    });
    let first = json_body(server.post_json("/command", envelope.clone()).await).await;
    let first_generation = first["generation"].as_u64().expect("generation");

    let replay = json_body(server.post_json("/api/command", envelope).await).await;
    assert_eq!(
        replay["generation"].as_u64().expect("generation"),
        first_generation,
        "a replay returns the original outcome"
    );

    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "accounts": {} }))
            .await,
    )
    .await;
    assert_eq!(
        body["data"]["rows"].as_array().expect("rows").len(),
        1,
        "the command applied exactly once"
    );

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_draft_command_is_visible_through_queries_at_the_returned_generation() {
    let server = TestServer::spawn().await;

    let accepted = json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-account-2",
                    "command": { "createAccount": { "name": "Lin" } }
                }),
            )
            .await,
    )
    .await;
    assert!(accepted["generation"].as_u64().is_some());

    let accounts = json_body(
        server
            .post_json("/query", serde_json::json!({ "accounts": {} }))
            .await,
    )
    .await;
    let account_id = accounts["data"]["rows"][0]["id"]
        .as_str()
        .expect("account id")
        .to_string();

    let accepted = json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-draft-1",
                    "command": { "createDraft": {
                        "accountId": account_id,
                        "draft": {
                            "from": null,
                            "to": [{ "name": null, "email": "to@example.com" }],
                            "cc": [],
                            "bcc": [],
                            "subject": "Hello",
                            "body": "A queued draft is a visible draft.",
                            "inReplyTo": null,
                            "references": null,
                        }
                    } }
                }),
            )
            .await,
    )
    .await;
    let draft_generation = accepted["generation"].as_u64().expect("generation");

    // The queued intent is queryable state (the outbox with verdicts).
    let pending = json_body(
        server
            .post_json(
                "/api/query",
                serde_json::json!({ "pendingOperations": { "accountId": account_id } }),
            )
            .await,
    )
    .await;
    assert!(pending["generation"].as_u64().expect("generation") >= draft_generation);
    let rows = pending["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "draftCreate");
    assert_eq!(rows[0]["state"], "pending");

    // The draft's local effect is visible in the mail list (instant drafts).
    let list = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailList": { "accountId": account_id } }),
            )
            .await,
    )
    .await;
    let rows = list["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["subject"], "Hello");

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_event_stream_hands_over_the_run_id_bumps_on_writes_and_heartbeats() {
    let server = TestServer::spawn().await;
    let mut stream = SseReader::connect(&server).await;

    // Handshake: the current generation plus the run id, so clients detect
    // restarts.
    let handshake = stream.next_data(Duration::from_secs(5)).await;
    let start_generation = handshake["generation"].as_u64().expect("generation");
    assert_eq!(
        handshake["runId"].as_str().expect("run id"),
        server.state.events.run_id()
    );

    // A write bumps the generation on the stream.
    let accepted = json_body(
        server
            .post_json(
                "/api/command",
                serde_json::json!({
                    "id": "cmd-account-3",
                    "command": { "createAccount": { "name": "Joan" } }
                }),
            )
            .await,
    )
    .await;
    let write_generation = accepted["generation"].as_u64().expect("generation");

    let message = stream.next_data(Duration::from_secs(5)).await;
    let streamed = message["generation"].as_u64().expect("generation");
    assert!(
        streamed > start_generation,
        "a write must move the streamed generation past the handshake"
    );
    let _ = write_generation;

    // Silence is filled by generation-only heartbeats (~5s apart): drain any
    // remaining write burst, then a payload-less message must arrive.
    let heartbeat = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let message = stream.next_data(Duration::from_secs(8)).await;
            if message.get("event").is_none() {
                break message;
            }
        }
    })
    .await
    .expect("a generation-only heartbeat fills the silence");
    assert!(heartbeat["generation"].as_u64().expect("generation") >= streamed);

    server.shutdown().await;
}
