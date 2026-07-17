//! Live end-to-end test of the backend HTTP + SSE surface against a real
//! Stalwart: a disposable server is spawned and seeded, a JMAP account is
//! pointed at it, and the whole read/write loop is driven exactly the way a
//! client drives it — queries and commands over HTTP, liveness off the event
//! stream. Gated on `POSTHASTE_STALWART_INTEGRATION=1` (real Stalwart
//! required); skipped otherwise.
//!
//! Convergence is observed the level-triggered way: the test refetches the
//! mail list whenever the event stream reports a new generation, never on a
//! bare sleep.

use std::sync::Arc;
use std::time::{Duration, Instant};

use posthaste_client_backend::{serve, AppPaths, AppState, ServerHandle};
use posthaste_domain_model::{AccountDriver, AccountId, AccountSettings};
use posthaste_testkit::{StalwartFixture, TestSecretStore};

/// Poll interval for the account runtime: short, so the test converges via
/// the poll safety net even when push delivery is slow.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Overall budget for the injected messages to converge into the list.
const CONVERGENCE_DEADLINE: Duration = Duration::from_secs(120);

/// Budget for one SSE frame. Heartbeats arrive every ~5s, so a silent
/// stream inside this window is a broken stream.
const SSE_FRAME_DEADLINE: Duration = Duration::from_secs(20);

fn integration_enabled() -> bool {
    std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() == Ok("1")
}

/// The backend under test: assembled state + the HTTP server on an ephemeral
/// loopback port, over temporary config/state roots.
struct LiveServer {
    state: AppState,
    server: ServerHandle,
    token: String,
    http: reqwest::Client,
    /// Owns the config/state roots for the server's lifetime.
    _dir: tempfile::TempDir,
}

impl LiveServer {
    async fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));
        let mut options = posthaste_client_backend::BuildOptions::at(paths);
        options.poll_interval = POLL_INTERVAL;
        options.secret_store = Some(Arc::new(TestSecretStore::default()));
        let state = AppState::assemble(options).await.expect("assemble backend");
        let token = "live-session-token".to_string();
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

    async fn post_json(&self, path: &str, body: &serde_json::Value) -> serde_json::Value {
        let response = self
            .http
            .post(self.url(path))
            .header("authorization", format!("Bearer {}", self.token))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("request completes");
        let status = response.status();
        let text = response.text().await.expect("read body");
        assert_eq!(status, 200, "unexpected status for {body}: {text}");
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("invalid JSON ({error}): {text}"))
    }

    async fn query(&self, query: serde_json::Value) -> serde_json::Value {
        self.post_json("/api/query", &query).await
    }

    async fn command(&self, envelope: &serde_json::Value) -> serde_json::Value {
        self.post_json("/api/command", envelope).await
    }

    /// One page of the account's inbox-scoped mail list:
    /// `(answer generation, rows)`.
    async fn mail_list(&self, account_id: &str, mailbox_id: &str) -> (u64, Vec<serde_json::Value>) {
        let answer = self
            .query(serde_json::json!({ "mailList": {
                "accountId": account_id,
                "mailboxId": mailbox_id,
                "limit": 200,
            } }))
            .await;
        let generation = answer["generation"].as_u64().expect("generation stamp");
        let rows = answer["data"]["rows"]
            .as_array()
            .expect("mail list rows")
            .clone();
        (generation, rows)
    }

    /// The `(mailbox id, unread, total)` of the account mailbox carrying
    /// `role`, from the mailbox-counts query.
    async fn mailbox_by_role(&self, account_id: &str, role: &str) -> (String, i64, i64) {
        let answer = self
            .query(serde_json::json!({ "mailboxCounts": { "accountId": account_id } }))
            .await;
        let rows = answer["data"]["rows"].as_array().expect("mailbox rows");
        let mailbox = rows
            .iter()
            .map(|row| &row["mailbox"])
            .find(|mailbox| {
                mailbox["role"]
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case(role))
            })
            .unwrap_or_else(|| panic!("no mailbox with role {role} in {rows:?}"));
        (
            mailbox["id"].as_str().expect("mailbox id").to_string(),
            mailbox["unreadEmails"].as_i64().expect("unread count"),
            mailbox["totalEmails"].as_i64().expect("total count"),
        )
    }

    async fn shutdown(self) {
        self.server.abort();
        self.state.shutdown().await;
    }
}

/// Create an enabled JMAP account pointed at the fixture and run its initial
/// sync to completion.
///
/// This goes through direct service/supervisor calls — the same path the
/// command endpoint's `createAccount` uses — because the command surface
/// deliberately carries only identity fields: driver, transport, and secrets
/// belong to the settings surface, which is not part of the command
/// vocabulary. The password lands in the injected in-memory secret store
/// under the same secret reference the account transport carries.
async fn create_jmap_account(server: &LiveServer, stalwart: &StalwartFixture) -> AccountId {
    let transport = stalwart.jmap_transport();
    let secret_ref = transport
        .secret_ref
        .clone()
        .expect("fixture transport carries a secret reference");
    server
        .state
        .secret_store
        .save(&secret_ref, &stalwart.password)
        .expect("store the account password");

    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format timestamp");
    let settings = AccountSettings {
        id: AccountId::from("jmap-live"),
        name: "jmap-live".to_string(),
        full_name: Some("Dev Account".to_string()),
        signature: None,
        email_patterns: vec![stalwart.email()],
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: None,
        transport,
        created_at: now.clone(),
        updated_at: now,
    };
    server
        .state
        .service
        .insert_source(&settings)
        .expect("insert account settings");
    server.state.supervisor.start_account(&settings).await;
    server
        .state
        .supervisor
        .sync_account(&settings.id)
        .await
        .expect("initial sync completes");
    settings.id
}

/// Reads SSE `data:` payloads off a streaming response.
struct SseReader {
    response: reqwest::Response,
    buffer: String,
}

impl SseReader {
    async fn connect(server: &LiveServer) -> Self {
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
        .expect("SSE frame within the deadline")
    }
}

fn row_ids(rows: &[serde_json::Value]) -> Vec<&str> {
    rows.iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<Vec<_>>()
}

#[tokio::test(flavor = "multi_thread")]
async fn live_backend_serves_the_full_loop_against_stalwart() {
    if !integration_enabled() {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    // Spawning + seeding Stalwart is blocking process work.
    let stalwart = tokio::task::spawn_blocking(StalwartFixture::start)
        .await
        .expect("fixture task");
    let server = LiveServer::spawn().await;
    let account = create_jmap_account(&server, &stalwart).await;
    let account_id = account.as_str();

    // --- Initial sync: the seeded messages are served through /query. ------
    let (inbox_id, _, seeded_total) = server.mailbox_by_role(account_id, "inbox").await;
    assert!(
        seeded_total > 0,
        "the seeded inbox must not be empty after the initial sync"
    );
    let (_, seeded_rows) = server.mail_list(account_id, &inbox_id).await;
    assert_eq!(
        seeded_rows.len(),
        seeded_total as usize,
        "the inbox list and the inbox counter must agree"
    );

    // --- Live convergence: 20 injections reach the list via sync + SSE. ----
    let mut stream = SseReader::connect(&server).await;
    let handshake = stream.next_data(SSE_FRAME_DEADLINE).await;
    let handshake_generation = handshake["generation"].as_u64().expect("generation");
    assert_eq!(
        handshake["runId"].as_str().expect("run id"),
        server.state.events.run_id(),
        "the handshake carries this run's id"
    );

    // 20 messages, in two batches: each `inject` call delivers over one SMTP
    // session, and Stalwart caps the messages accepted per session at 10.
    stalwart.inject(10).await;
    stalwart.inject(10).await;

    let expected = seeded_rows.len() + 20;
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    let mut refetched_at = handshake_generation;
    let mut inbox_rows = seeded_rows;
    while inbox_rows.len() < expected {
        assert!(
            Instant::now() < deadline,
            "inbox never reached {expected} rows (at {} when the deadline passed)",
            inbox_rows.len()
        );
        // Level-triggered liveness: refetch only when the stream reports a
        // generation past the last refetch — never on a bare sleep.
        let frame = stream.next_data(SSE_FRAME_DEADLINE).await;
        let generation = frame["generation"].as_u64().expect("generation");
        if generation <= refetched_at {
            continue;
        }
        refetched_at = generation;
        let (_, rows) = server.mail_list(account_id, &inbox_id).await;
        inbox_rows = rows;
    }
    assert!(
        refetched_at > handshake_generation,
        "sync writes must advance the streamed generation"
    );
    let (_, _, inbox_total) = server.mailbox_by_role(account_id, "inbox").await;
    assert_eq!(inbox_total as usize, expected);

    // --- Archive one message through /command. -----------------------------
    let (archive_id, _, archive_total_before) = server.mailbox_by_role(account_id, "archive").await;
    let target = inbox_rows[0]["id"]
        .as_str()
        .expect("target message id")
        .to_string();
    let envelope = serde_json::json!({
        "id": "cmd-live-archive-1",
        "command": { "replaceMailboxes": {
            "accountId": account_id,
            "messageId": target,
            "change": { "mailboxIds": [archive_id] },
        } }
    });
    let accepted = server.command(&envelope).await;
    let archive_generation = accepted["generation"].as_u64().expect("generation");
    assert!(
        archive_generation > handshake_generation,
        "the archive commit must advance the generation"
    );

    // The command's local effect reads back at (or past) the returned
    // generation: the row left the inbox and the counters moved with it.
    let (list_generation, rows) = server.mail_list(account_id, &inbox_id).await;
    assert!(
        list_generation >= archive_generation,
        "the answer must be stamped at or past the command's generation"
    );
    assert_eq!(rows.len(), expected - 1);
    assert!(
        !row_ids(&rows).contains(&target.as_str()),
        "the archived message must leave the inbox list"
    );
    let (_, _, inbox_total) = server.mailbox_by_role(account_id, "inbox").await;
    assert_eq!(inbox_total as usize, expected - 1);
    let (_, _, archive_total) = server.mailbox_by_role(account_id, "archive").await;
    assert_eq!(archive_total, archive_total_before + 1);

    // --- Idempotency: replaying the envelope returns the original outcome. -
    let replay = server.command(&envelope).await;
    assert_eq!(
        replay["generation"].as_u64().expect("generation"),
        archive_generation,
        "a replay returns the original outcome"
    );
    let (_, rows) = server.mail_list(account_id, &inbox_id).await;
    assert_eq!(rows.len(), expected - 1, "a replay never re-applies");
    let pending = server
        .query(serde_json::json!({ "pendingOperations": { "accountId": account_id } }))
        .await;
    let targeting = pending["data"]["rows"]
        .as_array()
        .expect("pending rows")
        .iter()
        .filter(|row| row["entityId"].as_str() == Some(target.as_str()))
        .count();
    assert!(
        targeting <= 1,
        "the outbox must hold at most one intent for the archived message, found {targeting}"
    );

    server.shutdown().await;
    drop(stalwart);
}
