//! HTTP-level tests for the API surface: a real backend assembled over
//! temporary directories, served on an ephemeral loopback port, and driven
//! through plain HTTP — the same way every client consumes it.

use std::sync::Arc;
use std::time::Duration;

use posthaste_client_backend::{serve, AppPaths, AppState, BuildError, BuildOptions, ServerHandle};
use posthaste_domain_model::{
    now_iso8601, AccountDriver, AccountId, AccountSettings, AccountStatus,
    AccountTransportSettings, ProviderAuthKind, ProviderHint, SecretKind, SecretRef,
    SecretStoreError,
};
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
        Self::spawn_with_secret_store(Arc::new(TestSecretStore)).await
    }

    async fn spawn_with_secret_store(secret_store: Arc<dyn SecretStore>) -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));
        let mut options = BuildOptions::at(paths);
        options.secret_store = Some(secret_store);
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

/// Configure and start a mock-provider account, waiting until its startup
/// sync settles so mailboxes and messages are queryable.
async fn spawn_mock_account(server: &TestServer, id: &str) {
    let now = now_iso8601().expect("clock");
    let account = AccountSettings {
        id: id.into(),
        name: format!("Mock {id}"),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: now.clone(),
        updated_at: now,
    };
    server
        .state
        .config
        .save_source(&account)
        .expect("save account");
    server
        .state
        .service
        .sync_source_projections()
        .expect("project sources");
    server.state.supervisor.start_account(&account).await;
    let account_id = AccountId::from(id);
    for _ in 0..200 {
        let status = server
            .state
            .supervisor
            .runtime_overview(&account_id)
            .await
            .status;
        if status == AccountStatus::Ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("mock account {id} never reached ready");
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

/// A rule whose single condition matches subjects containing `text`.
fn subject_contains_rule(text: &str) -> serde_json::Value {
    serde_json::json!({ "root": { "operator": "all", "negated": false, "nodes": [
        { "type": "condition", "field": "subject", "operator": "contains",
          "negated": false, "value": text }
    ]}})
}

#[tokio::test(flavor = "multi_thread")]
async fn smart_mailbox_crud_drives_the_list_and_scopes_the_mail_list() {
    let server = TestServer::spawn().await;

    // A fresh config carries the built-in defaults, rules included.
    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    let defaults = body["data"]["rows"].as_array().expect("rows").clone();
    assert!(defaults.iter().any(|row| row["defaultKey"] == "inbox"));
    assert!(defaults.iter().all(|row| row["rule"]["root"].is_object()));

    // Create a user smart mailbox; it appears in the next answer.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-create",
                "command": { "createSmartMailbox": {
                    "name": "Greetings",
                    "rule": subject_contains_rule("Hello"),
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);

    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    let rows = body["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), defaults.len() + 1);
    let created = rows
        .iter()
        .find(|row| row["name"] == "Greetings")
        .expect("created smart mailbox is listed");
    assert_eq!(created["kind"], "user");
    assert_eq!(created["totalMessages"], 0);
    let smart_mailbox_id = created["id"].as_str().expect("id").to_string();

    // Seed one matching message: an account with an instant draft.
    json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-smb-account",
                    "command": { "createAccount": { "name": "Ada" } }
                }),
            )
            .await,
    )
    .await;
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
    json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-smb-draft",
                    "command": { "createDraft": {
                        "accountId": account_id,
                        "draft": {
                            "from": null,
                            "to": [{ "name": null, "email": "to@example.com" }],
                            "cc": [], "bcc": [],
                            "subject": "Hello",
                            "body": "Rule-scoped lists see this draft.",
                            "inReplyTo": null, "references": null,
                        }
                    } }
                }),
            )
            .await,
    )
    .await;

    // The smart-mailbox scope evaluates the saved rule.
    let list = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailList": { "smartMailboxId": smart_mailbox_id } }),
            )
            .await,
    )
    .await;
    let list_rows = list["data"]["rows"].as_array().expect("rows");
    assert_eq!(list_rows.len(), 1);
    assert_eq!(list_rows[0]["subject"], "Hello");

    // The row's counts fold the same rule.
    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    let created = body["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["name"] == "Greetings")
        .cloned()
        .expect("created smart mailbox is listed");
    assert_eq!(created["totalMessages"], 1);

    // The two mailbox scopes are mutually exclusive.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "mailList": {
                "smartMailboxId": smart_mailbox_id, "mailboxId": "mb-anything"
            } }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // An unknown smart-mailbox scope is an unknown id.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "mailList": { "smartMailboxId": "ghost" } }),
        )
        .await;
    assert_eq!(response.status(), 404);

    // Update: rename and swap the rule; the list reflects both.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-update",
                "command": { "updateSmartMailbox": {
                    "smartMailboxId": smart_mailbox_id,
                    "name": "No Matches",
                    "rule": subject_contains_rule("zzz-nothing"),
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let list = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailList": { "smartMailboxId": smart_mailbox_id } }),
            )
            .await,
    )
    .await;
    assert_eq!(list["data"]["rows"].as_array().expect("rows").len(), 0);

    // Guard rails: empty name, unassignable role, unknown update target.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-bad-name",
                "command": { "createSmartMailbox": {
                    "name": "  ", "rule": subject_contains_rule("x")
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-bad-role",
                "command": { "createSmartMailbox": {
                    "name": "Snoozed", "role": "snooze",
                    "rule": subject_contains_rule("x")
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-update-ghost",
                "command": { "updateSmartMailbox": {
                    "smartMailboxId": "ghost", "name": "Nope"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    // Delete removes it; a second delete is an unknown id.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-delete",
                "command": { "deleteSmartMailbox": { "smartMailboxId": smart_mailbox_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    assert!(body["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .all(|row| row["id"] != smart_mailbox_id.as_str()));
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-delete-again",
                "command": { "deleteSmartMailbox": { "smartMailboxId": smart_mailbox_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    // Reset restores a deleted built-in.
    let trash_id = defaults
        .iter()
        .find(|row| row["defaultKey"] == "trash")
        .and_then(|row| row["id"].as_str())
        .expect("trash default")
        .to_string();
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-delete-trash",
                "command": { "deleteSmartMailbox": { "smartMailboxId": trash_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-smb-reset",
                "command": { "resetSmartMailboxes": {} }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    assert!(body["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .any(|row| row["defaultKey"] == "trash"));

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mailbox_create_rename_role_and_delete_round_trip_through_the_provider() {
    let server = TestServer::spawn().await;
    spawn_mock_account(&server, "m1").await;

    // The startup sync surfaced the provider's mailboxes.
    let counts = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailboxCounts": { "accountId": "m1" } }),
            )
            .await,
    )
    .await;
    assert!(counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .any(|row| row["mailbox"]["name"] == "Inbox"));

    // Create a mailbox; the post-create resync makes it queryable at once.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-create",
                "command": { "createMailbox": { "accountId": "m1", "name": "Projects" } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let counts = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailboxCounts": { "accountId": "m1" } }),
            )
            .await,
    )
    .await;
    let projects = counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["mailbox"]["name"] == "Projects")
        .cloned()
        .expect("created mailbox is listed");
    let projects_id = projects["mailbox"]["id"].as_str().expect("id").to_string();
    assert_eq!(projects["mailbox"]["role"], serde_json::Value::Null);

    // Assign a role; the resynced projection carries it.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-role",
                "command": { "setMailboxRole": {
                    "accountId": "m1", "mailboxId": projects_id, "role": "junk"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let counts = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailboxCounts": { "accountId": "m1" } }),
            )
            .await,
    )
    .await;
    let projects = counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["mailbox"]["id"] == projects_id.as_str())
        .cloned()
        .expect("mailbox still listed");
    assert_eq!(projects["mailbox"]["role"], "junk");

    // Rename the mailbox; the resynced projection carries the new name under
    // the SAME id, with the assigned role untouched and the counts intact.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-rename",
                "command": { "renameMailbox": {
                    "accountId": "m1", "mailboxId": projects_id, "name": "Receipts"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let counts = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailboxCounts": { "accountId": "m1" } }),
            )
            .await,
    )
    .await;
    let renamed = counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["mailbox"]["id"] == projects_id.as_str())
        .cloned()
        .expect("the renamed mailbox keeps its id");
    assert_eq!(renamed["mailbox"]["name"], "Receipts");
    assert_eq!(renamed["mailbox"]["role"], "junk");
    assert_eq!(renamed["mailbox"]["totalEmails"], 0);

    // A blank rename never reaches the provider.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-rename-blank",
                "command": { "renameMailbox": {
                    "accountId": "m1", "mailboxId": projects_id, "name": "   "
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // An unknown role never reaches the provider.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-bad-role",
                "command": { "setMailboxRole": {
                    "accountId": "m1", "mailboxId": projects_id, "role": "shoebox"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // Deleting a non-empty mailbox without the confirmed flag is a conflict.
    let inbox_id = counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["mailbox"]["role"] == "inbox")
        .and_then(|row| row["mailbox"]["id"].as_str())
        .expect("inbox")
        .to_string();
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-delete-inbox",
                "command": { "deleteMailbox": { "accountId": "m1", "mailboxId": inbox_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);
    let body = json_body(response).await;
    assert_eq!(body["kind"], "conflict");

    // The empty mailbox deletes cleanly and disappears from the counts.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-delete-projects",
                "command": { "deleteMailbox": { "accountId": "m1", "mailboxId": projects_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let counts = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "mailboxCounts": { "accountId": "m1" } }),
            )
            .await,
    )
    .await;
    assert!(counts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .all(|row| row["mailbox"]["id"] != projects_id.as_str()));

    // Renaming a mailbox that no longer exists is an unknown id, refused
    // before the provider is touched.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-rename-gone",
                "command": { "renameMailbox": {
                    "accountId": "m1", "mailboxId": projects_id, "name": "Postbox"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);
    let body = json_body(response).await;
    assert_eq!(body["kind"], "unknownId");

    // An unknown account is an unknown id, not a connection failure.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-mb-ghost-account",
                "command": { "createMailbox": { "accountId": "ghost", "name": "X" } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_enumerate_user_keywords_per_account_and_merged() {
    let server = TestServer::spawn().await;
    spawn_mock_account(&server, "t1").await;

    // No user keywords yet: the system `$seen`/`$flagged` never surface.
    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "tags": { "accountId": "t1" } }),
            )
            .await,
    )
    .await;
    assert_eq!(body["data"]["rows"], serde_json::json!([]));

    // Tag one read and one unread message with the same keyword.
    for (command_id, message_id) in [("cmd-tag-1", "em-001"), ("cmd-tag-2", "em-002")] {
        let response = server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": command_id,
                    "command": { "setKeywords": {
                        "accountId": "t1",
                        "messageId": message_id,
                        "change": { "add": ["project-x"], "remove": [] }
                    } }
                }),
            )
            .await;
        assert_eq!(response.status(), 200);
    }

    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "tags": { "accountId": "t1" } }),
            )
            .await,
    )
    .await;
    assert_eq!(
        body["data"]["rows"],
        serde_json::json!([
            { "name": "project-x", "unreadMessages": 1, "totalMessages": 2 }
        ])
    );

    // The unscoped query merges across accounts; with one account it is the
    // same set.
    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "tags": {} }))
            .await,
    )
    .await;
    assert_eq!(
        body["data"]["rows"],
        serde_json::json!([
            { "name": "project-x", "unreadMessages": 1, "totalMessages": 2 }
        ])
    );

    // An unknown account scope is an unknown id.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "tags": { "accountId": "ghost" } }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn app_settings_round_trip_writes_the_whole_document() {
    let server = TestServer::spawn().await;

    let body = json_body(
        server
            .post_json("/query", serde_json::json!({ "appSettings": {} }))
            .await,
    )
    .await;
    let first_generation = body["generation"].as_u64().expect("generation");
    let mut settings = body["data"]["settings"].clone();
    assert!(
        settings.is_object(),
        "the settings document is served whole"
    );

    // Read-modify-write: the client edits the document and writes it back
    // whole.
    settings["tags"] = serde_json::json!([{ "name": "work", "fg": "#112233" }]);
    settings["compose"] = serde_json::json!({ "undoSendDelaySeconds": 30 });
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-settings-1",
                "command": { "updateSettings": { "settings": settings } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let accepted = json_body(response).await;
    let write_generation = accepted["generation"].as_u64().expect("generation");
    assert!(write_generation > first_generation);

    let body = json_body(
        server
            .post_json("/api/query", serde_json::json!({ "appSettings": {} }))
            .await,
    )
    .await;
    assert!(body["generation"].as_u64().expect("generation") >= write_generation);
    assert_eq!(body["data"]["settings"]["tags"][0]["name"], "work");
    assert_eq!(
        body["data"]["settings"]["compose"]["undoSendDelaySeconds"],
        30
    );

    // An over-cap undo-send hold is rejected before anything is stored.
    let mut invalid = body["data"]["settings"].clone();
    invalid["compose"] = serde_json::json!({ "undoSendDelaySeconds": 999 });
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-settings-2",
                "command": { "updateSettings": { "settings": invalid } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);
    let error = json_body(response).await;
    assert_eq!(error["kind"], "malformedRequest");

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn account_lifecycle_covers_transport_secret_sync_and_delete() {
    let server = TestServer::spawn().await;

    let accepted = json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-lifecycle-create",
                    "command": { "createAccount": { "name": "Probe One" } }
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

    // Verification with no transport/credential fails the query with the
    // error envelope, never a fabricated "ok".
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "verifyAccount": { "accountId": account_id } }),
        )
        .await;
    assert_eq!(response.status(), 503);

    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-transport",
                "command": { "updateAccountTransport": {
                    "accountId": account_id,
                    "provider": "gmail",
                    "auth": "appPassword",
                    "username": { "kind": "set", "value": "probe@example.com" },
                    "imap": { "host": "imap.example.com", "port": 993, "security": "tls" },
                    "smtp": { "host": "smtp.example.com", "port": 587, "security": "startTls" },
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);

    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "accountSettings": { "accountId": account_id } }),
            )
            .await,
    )
    .await;
    let transport = &body["data"]["transport"];
    assert_eq!(transport["provider"], "gmail");
    assert_eq!(transport["username"], "probe@example.com");
    assert_eq!(transport["imap"]["host"], "imap.example.com");
    assert_eq!(transport["smtp"]["security"], "startTls");
    assert_eq!(transport["secret"]["configured"], false);

    // The one secret-bearing wire shape; the read side must never echo the
    // material back.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-secret",
                "command": { "setAccountSecret": {
                    "accountId": account_id,
                    "change": { "kind": "replace", "secret": "hunter2-app-password" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);

    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "accountSettings": { "accountId": account_id } }),
        )
        .await;
    let raw = response.text().await.expect("read body");
    assert!(
        !raw.contains("hunter2"),
        "secret material must never appear in a query answer"
    );
    let body: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(body["data"]["transport"]["secret"]["configured"], true);
    assert_eq!(body["data"]["transport"]["secret"]["storage"], "os");
    assert_eq!(
        body["data"]["transport"]["secret"]["label"],
        serde_json::Value::Null
    );

    // Keep is a no-op placeholder; clear removes the credential.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-keep",
                "command": { "setAccountSecret": {
                    "accountId": account_id,
                    "change": { "kind": "keep" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-clear",
                "command": { "setAccountSecret": {
                    "accountId": account_id,
                    "change": { "kind": "clear" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "accountSettings": { "accountId": account_id } }),
            )
            .await,
    )
    .await;
    assert_eq!(body["data"]["transport"]["secret"]["configured"], false);

    // A disabled account cannot be synced on demand.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-sync",
                "command": { "syncNow": { "accountId": account_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 503);

    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-lifecycle-delete",
                "command": { "deleteAccount": { "accountId": account_id } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let accounts = json_body(
        server
            .post_json("/query", serde_json::json!({ "accounts": {} }))
            .await,
    )
    .await;
    assert_eq!(accounts["data"]["rows"].as_array().expect("rows").len(), 0);
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "accountSettings": { "accountId": account_id } }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn update_patches_set_keep_and_clear_optional_fields() {
    async fn account_settings(server: &TestServer, account_id: &str) -> serde_json::Value {
        json_body(
            server
                .post_json(
                    "/query",
                    serde_json::json!({ "accountSettings": { "accountId": account_id } }),
                )
                .await,
        )
        .await
    }

    let server = TestServer::spawn().await;

    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-create",
                "command": { "createAccount": {
                    "name": "Patch Probe",
                    "fullName": "Ada Lovelace",
                    "signature": "-- Ada"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
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
    // Set replaces both identity fields.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-set",
                "command": { "updateAccount": {
                    "accountId": account_id,
                    "fullName": { "kind": "set", "value": "Ada K. Lovelace" },
                    "signature": { "kind": "set", "value": "-- Ada K." }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = account_settings(&server, &account_id).await;
    assert_eq!(body["data"]["fullName"], "Ada K. Lovelace");
    assert_eq!(body["data"]["signature"], "-- Ada K.");

    // Absent and explicit keep both preserve the stored values.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-keep",
                "command": { "updateAccount": {
                    "accountId": account_id,
                    "name": "Patch Probe Renamed",
                    "signature": { "kind": "keep" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = account_settings(&server, &account_id).await;
    assert_eq!(body["data"]["name"], "Patch Probe Renamed");
    assert_eq!(body["data"]["fullName"], "Ada K. Lovelace");
    assert_eq!(body["data"]["signature"], "-- Ada K.");

    // Clear nulls exactly the cleared field.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-clear",
                "command": { "updateAccount": {
                    "accountId": account_id,
                    "fullName": { "kind": "clear" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = account_settings(&server, &account_id).await;
    assert_eq!(body["data"]["fullName"], serde_json::Value::Null);
    assert_eq!(body["data"]["signature"], "-- Ada K.");

    // A bare null cannot clear: the request is malformed, nothing changes.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-null",
                "command": { "updateAccount": {
                    "accountId": account_id,
                    "signature": null
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // The same tristate drives the transport endpoints.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-transport-set",
                "command": { "updateAccountTransport": {
                    "accountId": account_id,
                    "baseUrl": { "kind": "set", "value": "https://jmap.example.com" },
                    "username": { "kind": "set", "value": "probe@example.com" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = account_settings(&server, &account_id).await;
    assert_eq!(
        body["data"]["transport"]["baseUrl"],
        "https://jmap.example.com"
    );
    assert_eq!(body["data"]["transport"]["username"], "probe@example.com");

    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-transport-clear",
                "command": { "updateAccountTransport": {
                    "accountId": account_id,
                    "baseUrl": { "kind": "clear" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = account_settings(&server, &account_id).await;
    assert_eq!(
        body["data"]["transport"]["baseUrl"],
        serde_json::Value::Null
    );
    assert_eq!(body["data"]["transport"]["username"], "probe@example.com");

    // The smart-mailbox role rides the same patch shape.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-smb-create",
                "command": { "createSmartMailbox": {
                    "name": "Patched", "role": "archive",
                    "rule": subject_contains_rule("x")
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let rows = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    let smart_mailbox = rows["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["name"] == "Patched")
        .cloned()
        .expect("created smart mailbox");
    assert_eq!(smart_mailbox["role"], "archive");
    let smart_mailbox_id = smart_mailbox["id"].as_str().expect("id").to_string();

    // Absent keeps the role; an explicit clear removes it.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-smb-keep",
                "command": { "updateSmartMailbox": {
                    "smartMailboxId": smart_mailbox_id, "name": "Patched Kept"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-patch-smb-clear",
                "command": { "updateSmartMailbox": {
                    "smartMailboxId": smart_mailbox_id,
                    "role": { "kind": "clear" }
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let rows = json_body(
        server
            .post_json("/query", serde_json::json!({ "smartMailboxes": {} }))
            .await,
    )
    .await;
    let smart_mailbox = rows["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["name"] == "Patched Kept")
        .cloned()
        .expect("updated smart mailbox");
    assert_eq!(smart_mailbox["role"], serde_json::Value::Null);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn account_logo_uploads_and_serves_through_the_asset_route() {
    let server = TestServer::spawn().await;

    let accepted = json_body(
        server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": "cmd-logo-create",
                    "command": { "createAccount": { "name": "Logo Owner" } }
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

    // "png-bytes", base64-encoded (the compose-attachment convention).
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-logo-upload",
                "command": { "setAccountLogo": {
                    "accountId": account_id,
                    "mimeType": "image/png",
                    "contentBase64": "cG5nLWJ5dGVz"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);

    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "accountSettings": { "accountId": account_id } }),
            )
            .await,
    )
    .await;
    assert_eq!(body["data"]["appearance"]["kind"], "image");
    let image_id = body["data"]["appearance"]["imageId"]
        .as_str()
        .expect("image id")
        .to_string();

    let response = server
        .http
        .get(server.url(&format!("/account-assets/logos/{image_id}")))
        .header("authorization", format!("Bearer {}", server.token))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let bytes = response.bytes().await.expect("read body");
    assert_eq!(&bytes[..], b"png-bytes");

    // An unsupported image type is rejected before anything lands on disk.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-logo-bad-mime",
                "command": { "setAccountLogo": {
                    "accountId": account_id,
                    "mimeType": "text/plain",
                    "contentBase64": "cG5nLWJ5dGVz"
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn oauth_start_mints_a_pkce_descriptor_without_touching_the_network() {
    let server = TestServer::spawn().await;

    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "oauthStart": {
                    "provider": "gmail",
                    "clientId": "client-1",
                    "redirectUri": "http://127.0.0.1:39999/oauth"
                } }),
            )
            .await,
    )
    .await;
    let authorization_url = body["data"]["authorizationUrl"]
        .as_str()
        .expect("authorization url");
    assert!(authorization_url.starts_with("https://accounts.google.com/"));
    assert!(authorization_url.contains("code_challenge="));
    assert!(authorization_url.contains("nonce="));
    assert!(!body["data"]["state"].as_str().expect("state").is_empty());
    assert_eq!(body["data"]["redirectUri"], "http://127.0.0.1:39999/oauth");

    // A provider without a built-in OAuth flow is rejected.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "oauthStart": {
                "provider": "icloud",
                "clientId": "client-1",
                "redirectUri": "http://127.0.0.1:39999/oauth"
            } }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // A callback for a state the backend never minted is rejected.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-oauth-bogus",
                "command": { "completeOauth": { "state": "not-a-state", "code": "code" } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    server.shutdown().await;
}

/// An OAuth account whose token refresh cannot succeed surfaces a typed auth
/// failure through the accounts query — health as state, never a hang. The
/// stored token set is expired and holds no refresh token, so the resolve
/// fails as a credential problem before any network is touched, and the
/// query keeps answering while the runtime records the failure.
#[tokio::test(flavor = "multi_thread")]
async fn oauth_refresh_failure_surfaces_as_auth_error_through_the_accounts_query() {
    let secret_store = Arc::new(posthaste_testkit::TestSecretStore::default());
    let server = TestServer::spawn_with_secret_store(secret_store.clone()).await;

    let secret_ref = SecretRef {
        kind: SecretKind::Os,
        key: "account:oauth-stale".to_string(),
    };
    let token_set = serde_json::json!({
        "type": "oauth2",
        "provider": "gmail",
        "clientId": "bundled-client-id",
        "accessToken": "expired-access-token",
        "refreshToken": null,
        "expiresAt": "2020-01-01T00:00:00Z",
        "scopes": ["https://mail.google.com/"],
    });
    secret_store
        .save(&secret_ref, &token_set.to_string())
        .expect("seed the stale token set");

    let now = now_iso8601().expect("clock");
    let account = AccountSettings {
        id: "oauth-stale".into(),
        name: "OAuth Stale".to_string(),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider: ProviderHint::Gmail,
            auth: ProviderAuthKind::OAuth2,
            secret_ref: Some(secret_ref),
            ..AccountTransportSettings::default()
        },
        created_at: now.clone(),
        updated_at: now,
    };
    server
        .state
        .config
        .save_source(&account)
        .expect("save account");
    server
        .state
        .service
        .sync_source_projections()
        .expect("project sources");
    server.state.supervisor.start_account(&account).await;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let body = json_body(
            server
                .post_json("/query", serde_json::json!({ "accounts": {} }))
                .await,
        )
        .await;
        let row = body["data"]["rows"]
            .as_array()
            .expect("account rows")
            .iter()
            .find(|row| row["id"] == "oauth-stale")
            .expect("the OAuth account row")
            .clone();
        if row["status"] == "authError" {
            assert!(
                row["lastSyncError"]
                    .as_str()
                    .is_some_and(|message| !message.is_empty()),
                "the auth failure carries a user-facing message: {row}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the account never surfaced the typed auth error; last row: {row}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    server.shutdown().await;
}

// ---------------------------------------------------------------------------
// Automation + time cluster: rules, snooze, rev-log undo/redo, outbox
// retry/cancel, raw source, sender addresses, unsubscribe.
// ---------------------------------------------------------------------------

use posthaste_domain_model::{
    ListUnsubscribe, MailboxId, MailboxRecord, MessageId, MessageRecord, OperationId,
    OperationState, Recipient, SyncBatch, ThreadId,
};
use posthaste_domain_service::BaseWrite;

/// Create a plain (disabled) account over HTTP and hand back its id.
async fn create_plain_account(server: &TestServer, command_id: &str, name: &str) -> AccountId {
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": command_id,
                "command": { "createAccount": { "name": name } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let accounts = json_body(
        server
            .post_json("/query", serde_json::json!({ "accounts": {} }))
            .await,
    )
    .await;
    let id = accounts["data"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["name"] == name)
        .expect("created account")["id"]
        .as_str()
        .expect("account id")
        .to_string();
    server
        .state
        .service
        .sync_source_projections()
        .expect("project sources");
    AccountId::from(id.as_str())
}

fn mailbox_record(id: &str, name: &str, role: Option<&str>) -> MailboxRecord {
    MailboxRecord {
        id: MailboxId::from(id),
        name: name.to_string(),
        role: role.map(str::to_string),
        unread_emails: 0,
        total_emails: 0,
    }
}

fn message_record(id: &str, from_email: &str, received_at: &str, mailbox: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(id),
        subject: Some(format!("Subject {id}")),
        from_email: Some(from_email.to_string()),
        received_at: received_at.to_string(),
        mailbox_ids: vec![MailboxId::from(mailbox)],
        ..MessageRecord::default()
    }
}

/// Seed provider truth directly through the store's sync-apply path: these
/// tests exercise the API handlers, not the provider sync.
fn seed_mail(
    server: &TestServer,
    account_id: &AccountId,
    mailboxes: Vec<MailboxRecord>,
    messages: Vec<MessageRecord>,
) {
    let batch = SyncBatch {
        mailboxes,
        messages,
        ..SyncBatch::default()
    };
    server
        .state
        .store
        .apply_sync_batch(
            &BaseWrite::legacy("api tests seed provider truth"),
            account_id,
            &batch,
        )
        .expect("seed sync batch");
}

/// One-condition rule tree matching `fromEmail contains value`.
fn from_email_condition(value: &str) -> serde_json::Value {
    serde_json::json!({
        "root": {
            "operator": "all",
            "negated": false,
            "nodes": [{
                "type": "condition",
                "field": "fromEmail",
                "operator": "contains",
                "negated": false,
                "value": value,
            }],
        }
    })
}

async fn message_summary(
    server: &TestServer,
    account: &AccountId,
    message: &str,
) -> serde_json::Value {
    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "messageDetail": {
                    "accountId": account.as_str(),
                    "messageId": message,
                } }),
            )
            .await,
    )
    .await;
    body["data"]["summary"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn automation_rule_crud_edits_the_settings_document_and_preview_matches_mail() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-auto-account", "Automation").await;
    seed_mail(
        &server,
        &account,
        vec![mailbox_record("inbox", "Inbox", Some("inbox"))],
        vec![
            message_record(
                "m-news",
                "news@example.com",
                "2026-07-01T00:00:00Z",
                "inbox",
            ),
            message_record(
                "m-friend",
                "friend@example.com",
                "2026-07-02T00:00:00Z",
                "inbox",
            ),
        ],
    );

    // The preview evaluates the condition over today's mail.
    let preview = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "automationRulePreview": {
                    "condition": from_email_condition("news"),
                    "limit": 10,
                } }),
            )
            .await,
    )
    .await;
    assert_eq!(preview["data"]["total"], 1);
    assert_eq!(preview["data"]["rows"][0]["fromEmail"], "news@example.com");

    let rule = serde_json::json!({
        "id": "rule-1",
        "name": "News",
        "enabled": true,
        "triggers": ["messageArrived"],
        "condition": from_email_condition("news"),
        "actions": [{ "kind": "markRead" }],
        "backfill": false,
    });
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-rule-create",
                "command": { "createAutomationRule": { "rule": rule } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let rules = server
        .state
        .service
        .get_app_settings()
        .expect("settings")
        .automation_rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].name, "News");

    // A duplicate id is a conflict, not a second rule.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-rule-create-duplicate",
                "command": { "createAutomationRule": { "rule": rule } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    // Update replaces the rule, keyed by its id.
    let mut renamed = rule.clone();
    renamed["name"] = serde_json::json!("News (renamed)");
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-rule-update",
                "command": { "updateAutomationRule": { "rule": renamed } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let rules = server
        .state
        .service
        .get_app_settings()
        .expect("settings")
        .automation_rules;
    assert_eq!(rules[0].name, "News (renamed)");

    // Updating an unknown rule is an unknown id.
    let mut unknown = rule.clone();
    unknown["id"] = serde_json::json!("rule-404");
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-rule-update-unknown",
                "command": { "updateAutomationRule": { "rule": unknown } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    // Delete empties the list and is idempotent.
    for command_id in ["cmd-rule-delete", "cmd-rule-delete-again"] {
        let response = server
            .post_json(
                "/command",
                serde_json::json!({
                    "id": command_id,
                    "command": { "deleteAutomationRule": { "ruleId": "rule-1" } }
                }),
            )
            .await;
        assert_eq!(response.status(), 200);
    }
    let rules = server
        .state
        .service
        .get_app_settings()
        .expect("settings")
        .automation_rules;
    assert!(rules.is_empty());

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn snooze_parks_the_message_in_the_snooze_mailbox_and_unsnooze_returns_it() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-snooze-account", "Snoozer").await;
    seed_mail(
        &server,
        &account,
        vec![
            mailbox_record("inbox", "Inbox", Some("inbox")),
            mailbox_record("later", "Later", Some("snooze")),
        ],
        vec![message_record(
            "m-1",
            "sender@example.com",
            "2026-07-01T00:00:00Z",
            "inbox",
        )],
    );

    // Park it until 2030: membership moves and the return time is recorded.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-snooze-1",
                "command": { "snooze": {
                    "accountId": account.as_str(),
                    "messageId": "m-1",
                    "until": "2030-01-01T00:00:00Z",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let summary = message_summary(&server, &account, "m-1").await;
    assert_eq!(summary["mailboxIds"], serde_json::json!(["later"]));
    let due = server
        .state
        .store
        .list_due_snoozes(&account, 4_102_444_800)
        .expect("list due snoozes");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0.as_str(), "m-1");
    assert_eq!(
        due[0].1, 1_893_456_000,
        "2030-01-01T00:00:00Z in unix seconds"
    );

    // Return it now: membership goes back and the snooze row clears.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-unsnooze-1",
                "command": { "unsnooze": {
                    "accountId": account.as_str(),
                    "messageId": "m-1",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let summary = message_summary(&server, &account, "m-1").await;
    assert_eq!(summary["mailboxIds"], serde_json::json!(["inbox"]));
    let due = server
        .state
        .store
        .list_due_snoozes(&account, 4_102_444_800)
        .expect("list due snoozes");
    assert!(due.is_empty(), "the mailbox replace cleared the snooze row");

    // A malformed wall time is rejected before anything moves.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-snooze-bad-time",
                "command": { "snooze": {
                    "accountId": account.as_str(),
                    "messageId": "m-1",
                    "until": "tomorrow-ish",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 400);

    // Without a snooze-role mailbox the command is a conflict.
    let bare = create_plain_account(&server, "cmd-snooze-bare-account", "Bare").await;
    seed_mail(
        &server,
        &bare,
        vec![mailbox_record("inbox", "Inbox", Some("inbox"))],
        vec![message_record(
            "m-2",
            "sender@example.com",
            "2026-07-01T00:00:00Z",
            "inbox",
        )],
    );
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-snooze-no-mailbox",
                "command": { "snooze": {
                    "accountId": bare.as_str(),
                    "messageId": "m-2",
                    "until": "2030-01-01T00:00:00Z",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_reverts_the_cursor_step_and_redo_reapplies_it() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-revlog-account", "History").await;
    let mut archived = message_record(
        "m-1",
        "sender@example.com",
        "2026-07-01T00:00:00Z",
        "archive",
    );
    archived.keywords = vec!["$flagged".to_string()];
    seed_mail(
        &server,
        &account,
        vec![
            mailbox_record("inbox", "Inbox", Some("inbox")),
            mailbox_record("archive", "Archive", Some("archive")),
        ],
        vec![archived],
    );
    // The recorded forward action: flag + archive (out of the inbox).
    let diff = serde_json::json!({
        "keywords": { "added": ["$flagged"], "removed": [] },
        "mailboxes": { "added": ["archive"], "removed": ["inbox"] },
    });
    server
        .state
        .store
        .append_rev_log_step(
            &account,
            "step-1",
            "m-1",
            account.as_str(),
            &diff,
            "2026-07-01T00:00:01Z",
        )
        .expect("append rev-log step");
    server
        .state
        .store
        .set_rev_cursor(&account, Some("step-1"), &[])
        .expect("set cursor");

    // The query serves the log with its cursor.
    let log = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "revLog": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    assert_eq!(log["data"]["steps"].as_array().expect("steps").len(), 1);
    assert_eq!(log["data"]["cursor"]["cursorStepId"], "step-1");

    // Undo reverts the step and moves the cursor down.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-undo-1",
                "command": { "undo": { "accountId": account.as_str() } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let summary = message_summary(&server, &account, "m-1").await;
    assert_eq!(summary["mailboxIds"], serde_json::json!(["inbox"]));
    assert_eq!(summary["keywords"], serde_json::json!([]));
    let log = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "revLog": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    assert_eq!(
        log["data"]["cursor"]["cursorStepId"],
        serde_json::Value::Null
    );
    assert_eq!(
        log["data"]["cursor"]["redoTail"],
        serde_json::json!(["step-1"])
    );

    // A second undo has nothing to move.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-undo-2",
                "command": { "undo": { "accountId": account.as_str() } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    // Redo re-applies the undone step and moves the cursor back up.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-redo-1",
                "command": { "redo": { "accountId": account.as_str() } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let summary = message_summary(&server, &account, "m-1").await;
    assert_eq!(summary["mailboxIds"], serde_json::json!(["archive"]));
    assert_eq!(summary["keywords"], serde_json::json!(["$flagged"]));
    let log = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "revLog": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    assert_eq!(log["data"]["cursor"]["cursorStepId"], "step-1");
    assert_eq!(log["data"]["cursor"]["redoTail"], serde_json::json!([]));

    // With an empty redo tail there is nothing to re-apply.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-redo-2",
                "command": { "redo": { "accountId": account.as_str() } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn outbox_operations_cancel_and_retry_through_commands() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-outbox-account", "Outbox").await;

    let draft = serde_json::json!({
        "from": null,
        "to": [{ "name": null, "email": "to@example.com" }],
        "cc": [],
        "bcc": [],
        "subject": "Queued",
        "body": "One pending operation.",
        "inReplyTo": null,
        "references": null,
    });
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-outbox-draft-1",
                "command": { "createDraft": { "accountId": account.as_str(), "draft": draft } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let pending = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "pendingOperations": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    let operation_id = pending["data"]["rows"][0]["id"]
        .as_str()
        .expect("operation id")
        .to_string();

    // A pending operation cannot be retried...
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-outbox-retry-pending",
                "command": { "retryOperation": {
                    "accountId": account.as_str(),
                    "operationId": operation_id,
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    // ...but a failed one can: it re-arms to pending with the error cleared.
    server
        .state
        .store
        .update_operation_state(
            &OperationId::from(operation_id.as_str()),
            OperationState::Failed,
            1,
            Some("provider said no"),
        )
        .expect("fail the operation");
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-outbox-retry-failed",
                "command": { "retryOperation": {
                    "accountId": account.as_str(),
                    "operationId": operation_id,
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let pending = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "pendingOperations": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    assert_eq!(pending["data"]["rows"][0]["state"], "pending");
    assert_eq!(
        pending["data"]["rows"][0]["lastError"],
        serde_json::Value::Null
    );

    // Cancel discards it; the row disappears from the outbox.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-outbox-cancel",
                "command": { "cancelOperation": {
                    "accountId": account.as_str(),
                    "operationId": operation_id,
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let pending = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "pendingOperations": { "accountId": account.as_str() } }),
            )
            .await,
    )
    .await;
    assert!(pending["data"]["rows"].as_array().expect("rows").is_empty());

    // A gone id is unknown for both commands.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-outbox-cancel-gone",
                "command": { "cancelOperation": {
                    "accountId": account.as_str(),
                    "operationId": operation_id,
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sender_addresses_serve_the_cached_corpus_scoped_by_account() {
    let server = TestServer::spawn().await;
    let first = create_plain_account(&server, "cmd-sender-account-1", "First").await;
    let second = create_plain_account(&server, "cmd-sender-account-2", "Second").await;
    server
        .state
        .store
        .remember_sender_address(
            &first,
            &Recipient {
                name: Some("Ada".to_string()),
                email: "ada@example.com".to_string(),
            },
        )
        .expect("remember first sender");
    server
        .state
        .store
        .remember_sender_address(
            &second,
            &Recipient {
                name: None,
                email: "grace@example.com".to_string(),
            },
        )
        .expect("remember second sender");

    let all = json_body(
        server
            .post_json("/query", serde_json::json!({ "senderAddresses": {} }))
            .await,
    )
    .await;
    assert_eq!(all["data"]["rows"].as_array().expect("rows").len(), 2);

    let scoped = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "senderAddresses": { "accountId": first.as_str() } }),
            )
            .await,
    )
    .await;
    let rows = scoped["data"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["email"], "ada@example.com");
    assert_eq!(rows[0]["name"], "Ada");
    assert_eq!(rows[0]["accountId"], first.as_str());

    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "senderAddresses": { "accountId": "no-such-account" } }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn message_raw_source_serves_cached_rfc822_bytes() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-raw-account", "Raw").await;
    let mut with_raw = message_record("m-raw", "raw@example.com", "2026-07-01T00:00:00Z", "inbox");
    with_raw.raw_mime = Some(
        "From: raw@example.com\r\nSubject: Raw source\r\n\r\nThe verbatim body.\r\n".to_string(),
    );
    seed_mail(
        &server,
        &account,
        vec![mailbox_record("inbox", "Inbox", Some("inbox"))],
        vec![
            with_raw,
            message_record(
                "m-bare",
                "bare@example.com",
                "2026-07-02T00:00:00Z",
                "inbox",
            ),
        ],
    );

    let body = json_body(
        server
            .post_json(
                "/query",
                serde_json::json!({ "messageRawSource": {
                    "accountId": account.as_str(),
                    "messageId": "m-raw",
                } }),
            )
            .await,
    )
    .await;
    let raw = body["data"]["raw"].as_str().expect("raw source");
    assert!(raw.contains("From: raw@example.com"));
    assert!(raw.contains("The verbatim body."));

    // An unknown message is an unknown id.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "messageRawSource": {
                "accountId": account.as_str(),
                "messageId": "m-missing",
            } }),
        )
        .await;
    assert_eq!(response.status(), 404);

    // A message with no cached raw and no reachable gateway is unavailable.
    let response = server
        .post_json(
            "/query",
            serde_json::json!({ "messageRawSource": {
                "accountId": account.as_str(),
                "messageId": "m-bare",
            } }),
        )
        .await;
    assert_eq!(response.status(), 503);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unsubscribe_requires_a_valid_one_click_target() {
    let server = TestServer::spawn().await;
    let account = create_plain_account(&server, "cmd-unsub-account", "Lists").await;
    let mut link_only = message_record(
        "m-link",
        "list@example.com",
        "2026-07-01T00:00:00Z",
        "inbox",
    );
    link_only.list_unsubscribe = Some(ListUnsubscribe {
        https: Some("https://lists.example.com/unsub".to_string()),
        mailto: None,
        one_click: false,
    });
    seed_mail(
        &server,
        &account,
        vec![mailbox_record("inbox", "Inbox", Some("inbox"))],
        vec![
            message_record(
                "m-plain",
                "person@example.com",
                "2026-07-02T00:00:00Z",
                "inbox",
            ),
            link_only,
        ],
    );

    // No unsubscribe target at all.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-unsub-plain",
                "command": { "unsubscribe": {
                    "accountId": account.as_str(),
                    "messageId": "m-plain",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    // A plain link without RFC 8058 one-click must not be POSTed to.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-unsub-link",
                "command": { "unsubscribe": {
                    "accountId": account.as_str(),
                    "messageId": "m-link",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 409);

    // An unknown message is an unknown id.
    let response = server
        .post_json(
            "/command",
            serde_json::json!({
                "id": "cmd-unsub-missing",
                "command": { "unsubscribe": {
                    "accountId": account.as_str(),
                    "messageId": "m-missing",
                } }
            }),
        )
        .await;
    assert_eq!(response.status(), 404);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn state_root_lock_refuses_a_second_backend() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));

    let mut options = BuildOptions::at(paths.clone());
    options.secret_store = Some(Arc::new(TestSecretStore));
    let first = AppState::assemble(options)
        .await
        .expect("assemble first backend");

    // A second backend over the same state root — a second desktop launch,
    // another channel's build, or the standalone binary — must be refused
    // before it opens the store.
    let mut options = BuildOptions::at(paths.clone());
    options.secret_store = Some(Arc::new(TestSecretStore));
    let error = match AppState::assemble(options).await {
        Ok(state) => {
            state.shutdown().await;
            panic!("second backend over a live store must be refused");
        }
        Err(error) => error,
    };
    assert!(
        matches!(error, BuildError::StoreLocked { .. }),
        "unexpected error: {error}"
    );

    // A shut-down backend frees the store even while its state handles are
    // still alive.
    first.shutdown().await;
    let mut options = BuildOptions::at(paths);
    options.secret_store = Some(Arc::new(TestSecretStore));
    let reopened = AppState::assemble(options)
        .await
        .expect("store reopens after the first backend is gone");
    reopened.shutdown().await;
}
