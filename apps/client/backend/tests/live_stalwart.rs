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
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AutomationBackfillJobStatus, OperationId,
    OperationState,
};
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

    /// The `(mailbox id, unread, total)` of the first account mailbox
    /// matching `matches`, from the mailbox-counts query. `wanted` labels the
    /// panic when nothing matches.
    async fn mailbox_where(
        &self,
        account_id: &str,
        wanted: &str,
        matches: impl Fn(&serde_json::Value) -> bool,
    ) -> (String, i64, i64) {
        let answer = self
            .query(serde_json::json!({ "mailboxCounts": { "accountId": account_id } }))
            .await;
        let rows = answer["data"]["rows"].as_array().expect("mailbox rows");
        let mailbox = rows
            .iter()
            .map(|row| &row["mailbox"])
            .find(|mailbox| matches(mailbox))
            .unwrap_or_else(|| panic!("no mailbox {wanted} in {rows:?}"));
        (
            mailbox["id"].as_str().expect("mailbox id").to_string(),
            mailbox["unreadEmails"].as_i64().expect("unread count"),
            mailbox["totalEmails"].as_i64().expect("total count"),
        )
    }

    /// The `(mailbox id, unread, total)` of the account mailbox carrying
    /// `role`, from the mailbox-counts query.
    async fn mailbox_by_role(&self, account_id: &str, role: &str) -> (String, i64, i64) {
        self.mailbox_where(account_id, &format!("with role {role}"), |mailbox| {
            mailbox["role"]
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(role))
        })
        .await
    }

    /// The `(mailbox id, unread, total)` of the account mailbox named `name`.
    async fn mailbox_by_name(&self, account_id: &str, name: &str) -> (String, i64, i64) {
        self.mailbox_where(account_id, &format!("named {name}"), |mailbox| {
            mailbox["name"].as_str() == Some(name)
        })
        .await
    }

    /// The account's outbox rows from the pending-operations query.
    async fn pending_rows(&self, account_id: &str) -> Vec<serde_json::Value> {
        let answer = self
            .query(serde_json::json!({ "pendingOperations": { "accountId": account_id } }))
            .await;
        answer["data"]["rows"]
            .as_array()
            .expect("pending rows")
            .clone()
    }

    /// The smart-mailbox row named `name` from the smart-mailboxes query.
    async fn smart_mailbox_by_name(&self, name: &str) -> serde_json::Value {
        let answer = self
            .query(serde_json::json!({ "smartMailboxes": {} }))
            .await;
        let rows = answer["data"]["rows"].as_array().expect("smart rows");
        rows.iter()
            .find(|row| row["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("no smart mailbox named {name} in {rows:?}"))
            .clone()
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
async fn create_jmap_account(server: &LiveServer, stalwart: &StalwartFixture) -> AccountSettings {
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
    settings
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

    /// The next streamed generation strictly past `seen` — the level-triggered
    /// wakeup every convergence loop below turns on. Panics past `deadline`.
    async fn generation_past(&mut self, seen: u64, deadline: Instant) -> u64 {
        loop {
            assert!(
                Instant::now() < deadline,
                "the streamed generation never advanced past {seen}"
            );
            let frame = self.next_data(SSE_FRAME_DEADLINE).await;
            let generation = frame["generation"].as_u64().expect("generation");
            if generation > seen {
                return generation;
            }
        }
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
    let account_id = account.id.as_str();

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

/// The newer surfaces over the same live loop: settings roundtrip, smart
/// mailboxes answering over live mail, mailbox create + move + counts,
/// snooze/unsnooze, sync-now, and the failed-operation retry path. Same
/// convergence discipline as the loop test above: every wait is
/// level-triggered on a streamed generation advance, never a bare sleep.
#[tokio::test(flavor = "multi_thread")]
async fn live_backend_serves_the_new_surfaces_against_stalwart() {
    if !integration_enabled() {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = tokio::task::spawn_blocking(StalwartFixture::start)
        .await
        .expect("fixture task");
    let server = LiveServer::spawn().await;
    let account = create_jmap_account(&server, &stalwart).await;
    let account_id = account.id.as_str();

    let mut stream = SseReader::connect(&server).await;
    let handshake = stream.next_data(SSE_FRAME_DEADLINE).await;
    let mut seen_generation = handshake["generation"].as_u64().expect("generation");

    // --- Settings: write the document whole, read it back verbatim. --------
    let settings_before = server.query(serde_json::json!({ "appSettings": {} })).await;
    let mut settings_document = settings_before["data"]["settings"].clone();
    settings_document["compose"] = serde_json::json!({ "undoSendDelaySeconds": 45 });
    let settings_envelope = serde_json::json!({
        "id": "cmd-live-settings-1",
        "command": { "updateSettings": { "settings": settings_document } }
    });
    let accepted = server.command(&settings_envelope).await;
    let settings_generation = accepted["generation"].as_u64().expect("generation");
    assert!(
        settings_generation > seen_generation,
        "the settings write must advance the generation"
    );
    let settings_after = server.query(serde_json::json!({ "appSettings": {} })).await;
    assert!(
        settings_after["generation"].as_u64().expect("generation") >= settings_generation,
        "the answer must be stamped at or past the settings write"
    );
    assert_eq!(
        settings_after["data"]["settings"]["compose"]["undoSendDelaySeconds"], 45,
        "the written compose preference must read back"
    );
    let replay = server.command(&settings_envelope).await;
    assert_eq!(
        replay["generation"].as_u64().expect("generation"),
        settings_generation,
        "a settings-write replay returns the original outcome"
    );

    // --- Smart mailboxes: a saved rule answers over live mail. -------------
    let rule = serde_json::json!({ "root": {
        "operator": "all",
        "negated": false,
        "nodes": [{
            "type": "condition",
            "field": "subject",
            "operator": "contains",
            "negated": false,
            "value": "Injected",
        }],
    } });
    server
        .command(&serde_json::json!({
            "id": "cmd-live-smart-1",
            "command": { "createSmartMailbox": { "name": "Live Injected", "rule": rule } }
        }))
        .await;
    let smart_row = server.smart_mailbox_by_name("Live Injected").await;
    let smart_id = smart_row["id"]
        .as_str()
        .expect("smart mailbox id")
        .to_string();
    assert_eq!(
        smart_row["rule"], rule,
        "the saved rule reads back verbatim"
    );
    let smart_baseline = smart_row["totalMessages"].as_i64().expect("total");
    assert_eq!(
        smart_baseline, 0,
        "no seeded message carries the Injected subject"
    );

    stalwart.inject(5).await;
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    let mut smart_total = smart_baseline;
    while smart_total < smart_baseline + 5 {
        seen_generation = stream.generation_past(seen_generation, deadline).await;
        smart_total = server.smart_mailbox_by_name("Live Injected").await["totalMessages"]
            .as_i64()
            .expect("total");
    }
    assert_eq!(smart_total, smart_baseline + 5);
    let smart_list = server
        .query(serde_json::json!({ "mailList": { "smartMailboxId": smart_id, "limit": 200 } }))
        .await;
    let smart_rows = smart_list["data"]["rows"].as_array().expect("rows");
    assert_eq!(
        smart_rows.len(),
        5,
        "the smart-mailbox list and the smart-mailbox counter must agree"
    );
    assert!(
        smart_rows.iter().all(|row| {
            row["subject"]
                .as_str()
                .is_some_and(|subject| subject.contains("Injected"))
        }),
        "every row answered under the rule scope must match the rule"
    );

    // --- Mailbox create + move: counts move locally, then hold through the
    // provider settlement (no ghost row after the op retires). --------------
    server
        .command(&serde_json::json!({
            "id": "cmd-live-mailbox-1",
            "command": { "createMailbox": { "accountId": account_id, "name": "Live Reports" } }
        }))
        .await;
    let (reports_id, _, reports_total) = server.mailbox_by_name(account_id, "Live Reports").await;
    assert_eq!(reports_total, 0, "a freshly created mailbox starts empty");

    let (inbox_id, _, inbox_total_before) = server.mailbox_by_role(account_id, "inbox").await;
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    let moved = inbox_rows[0]["id"]
        .as_str()
        .expect("message id")
        .to_string();
    server
        .command(&serde_json::json!({
            "id": "cmd-live-move-1",
            "command": { "replaceMailboxes": {
                "accountId": account_id,
                "messageId": moved,
                "change": { "mailboxIds": [reports_id] },
            } }
        }))
        .await;
    let (_, reports_rows) = server.mail_list(account_id, &reports_id).await;
    assert_eq!(row_ids(&reports_rows), vec![moved.as_str()]);
    let (_, _, reports_total) = server.mailbox_by_name(account_id, "Live Reports").await;
    assert_eq!(reports_total, 1, "the moved message counts immediately");
    let (_, _, inbox_total) = server.mailbox_by_role(account_id, "inbox").await;
    assert_eq!(inbox_total, inbox_total_before - 1);

    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    loop {
        let outstanding = server
            .pending_rows(account_id)
            .await
            .iter()
            .any(|row| row["entityId"].as_str() == Some(moved.as_str()));
        if !outstanding {
            break;
        }
        seen_generation = stream.generation_past(seen_generation, deadline).await;
    }
    let (_, reports_rows) = server.mail_list(account_id, &reports_id).await;
    assert_eq!(
        row_ids(&reports_rows),
        vec![moved.as_str()],
        "the settled move must not duplicate or drop the row"
    );
    let (_, _, reports_total) = server.mailbox_by_name(account_id, "Live Reports").await;
    assert_eq!(reports_total, 1, "the count holds through settlement");

    // --- Rename: the provider applies the name-only update synchronously;
    // the id, the counts, and the message list all hold. --------------------
    let accepted = server
        .command(&serde_json::json!({
            "id": "cmd-live-rename-1",
            "command": { "renameMailbox": {
                "accountId": account_id,
                "mailboxId": reports_id,
                "name": "Live Reports Renamed",
            } }
        }))
        .await;
    assert!(
        accepted["generation"].as_u64().expect("generation") > seen_generation,
        "the rename must advance the generation"
    );
    let (renamed_id, _, renamed_total) = server
        .mailbox_by_name(account_id, "Live Reports Renamed")
        .await;
    assert_eq!(renamed_id, reports_id, "a rename keeps the mailbox id");
    assert_eq!(renamed_total, 1, "the count survives the rename");
    let counts = server
        .query(serde_json::json!({ "mailboxCounts": { "accountId": account_id } }))
        .await;
    assert!(
        counts["data"]["rows"]
            .as_array()
            .expect("mailbox rows")
            .iter()
            .all(|row| row["mailbox"]["name"].as_str() != Some("Live Reports")),
        "the old name must leave the mailbox list"
    );
    let (_, renamed_rows) = server.mail_list(account_id, &reports_id).await;
    assert_eq!(
        row_ids(&renamed_rows),
        vec![moved.as_str()],
        "the renamed mailbox's messages stay queryable"
    );

    // --- Snooze: a snooze-role mailbox hides the message from the inbox;
    // unsnooze restores it. -------------------------------------------------
    server
        .command(&serde_json::json!({
            "id": "cmd-live-snooze-mailbox-1",
            "command": { "createMailbox": { "accountId": account_id, "name": "Live Snoozed" } }
        }))
        .await;
    let (snoozed_id, _, _) = server.mailbox_by_name(account_id, "Live Snoozed").await;
    server
        .command(&serde_json::json!({
            "id": "cmd-live-snooze-role-1",
            "command": { "setMailboxRole": {
                "accountId": account_id,
                "mailboxId": snoozed_id,
                "role": "snooze",
            } }
        }))
        .await;
    let (snooze_role_id, _, _) = server.mailbox_by_role(account_id, "snooze").await;
    assert_eq!(
        snooze_role_id, snoozed_id,
        "the assigned snooze role must read back from the counts query"
    );

    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    let napper = inbox_rows[0]["id"]
        .as_str()
        .expect("message id")
        .to_string();
    let until = (time::OffsetDateTime::now_utc() + time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format snooze time");
    server
        .command(&serde_json::json!({
            "id": "cmd-live-snooze-1",
            "command": { "snooze": {
                "accountId": account_id,
                "messageId": napper,
                "until": until,
            } }
        }))
        .await;
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    assert!(
        !row_ids(&inbox_rows).contains(&napper.as_str()),
        "a snoozed message leaves the inbox list"
    );
    let (_, snoozed_rows) = server.mail_list(account_id, &snoozed_id).await;
    assert!(
        row_ids(&snoozed_rows).contains(&napper.as_str()),
        "a snoozed message parks in the snooze mailbox"
    );

    server
        .command(&serde_json::json!({
            "id": "cmd-live-unsnooze-1",
            "command": { "unsnooze": { "accountId": account_id, "messageId": napper } }
        }))
        .await;
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    assert!(
        row_ids(&inbox_rows).contains(&napper.as_str()),
        "an unsnoozed message returns to the inbox list"
    );
    let (_, snoozed_rows) = server.mail_list(account_id, &snoozed_id).await;
    assert!(
        !row_ids(&snoozed_rows).contains(&napper.as_str()),
        "an unsnoozed message leaves the snooze mailbox"
    );

    // The two moves settle at the provider before the next scenario reads
    // sync state; the returned message must survive its own settlement.
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    loop {
        let outstanding = server
            .pending_rows(account_id)
            .await
            .iter()
            .any(|row| row["entityId"].as_str() == Some(napper.as_str()));
        if !outstanding {
            break;
        }
        seen_generation = stream.generation_past(seen_generation, deadline).await;
    }
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    assert!(
        row_ids(&inbox_rows).contains(&napper.as_str()),
        "the unsnoozed message stays in the inbox through settlement"
    );

    // --- Sync-now: the requested full cycle completes observably. ----------
    server
        .command(&serde_json::json!({
            "id": "cmd-live-sync-1",
            "command": { "syncNow": { "accountId": account_id, "mode": "fullMetadata" } }
        }))
        .await;
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    loop {
        assert!(
            Instant::now() < deadline,
            "no fullMetadata sync.completed event arrived on the stream"
        );
        let frame = stream.next_data(SSE_FRAME_DEADLINE).await;
        seen_generation = frame["generation"].as_u64().expect("generation");
        let event = &frame["event"];
        if event["kind"].as_str() == Some("sync.completed")
            && event["accountId"].as_str() == Some(account_id)
            && event["payload"]["mode"].as_str() == Some("fullMetadata")
        {
            break;
        }
    }
    let accounts = server.query(serde_json::json!({ "accounts": {} })).await;
    let account_row = accounts["data"]["rows"]
        .as_array()
        .expect("account rows")
        .iter()
        .find(|row| row["id"].as_str() == Some(account_id))
        .expect("the live account row")
        .clone();
    assert!(
        account_row["lastSyncAt"].is_string(),
        "a completed sync stamps lastSyncAt"
    );
    assert!(
        account_row["lastSyncError"].is_null(),
        "a clean sync leaves no lastSyncError"
    );

    // --- Retry: a failed operation re-arms and settles. --------------------
    // The failure itself is injected at the store (the runtime is stopped so
    // no flusher races the injection); everything around it — the enqueue,
    // the failed-state read, the retry command, the flush after restart — is
    // the real path.
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    let flag_target = inbox_rows
        .iter()
        .find(|row| row["isFlagged"] == false)
        .expect("an unflagged inbox message")["id"]
        .as_str()
        .expect("message id")
        .to_string();
    server.state.supervisor.stop_account(&account.id).await;
    server
        .command(&serde_json::json!({
            "id": "cmd-live-flag-1",
            "command": { "setKeywords": {
                "accountId": account_id,
                "messageId": flag_target,
                "change": { "add": ["$flagged"], "remove": [] },
            } }
        }))
        .await;
    let operation_id = server
        .pending_rows(account_id)
        .await
        .iter()
        .find(|row| {
            row["entityId"].as_str() == Some(flag_target.as_str())
                && row["state"].as_str() == Some("pending")
        })
        .expect("the queued keyword operation")["id"]
        .as_str()
        .expect("operation id")
        .to_string();
    server
        .state
        .store
        .update_operation_state(
            &OperationId::from(operation_id.as_str()),
            OperationState::Failed,
            1,
            Some("connection lost mid-flush"),
        )
        .expect("inject the failure");
    let failed_row = server
        .pending_rows(account_id)
        .await
        .iter()
        .find(|row| row["id"].as_str() == Some(operation_id.as_str()))
        .expect("the failed operation row")
        .clone();
    assert_eq!(failed_row["state"].as_str(), Some("failed"));
    assert_eq!(
        failed_row["lastError"].as_str(),
        Some("connection lost mid-flush")
    );

    server.state.supervisor.start_account(&account).await;
    let accepted = server
        .command(&serde_json::json!({
            "id": "cmd-live-retry-1",
            "command": { "retryOperation": {
                "accountId": account_id,
                "operationId": operation_id,
            } }
        }))
        .await;
    assert!(
        accepted["generation"].as_u64().expect("generation") > seen_generation,
        "the retry must advance the generation"
    );
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    loop {
        let outstanding = server
            .pending_rows(account_id)
            .await
            .iter()
            .any(|row| row["id"].as_str() == Some(operation_id.as_str()));
        if !outstanding {
            break;
        }
        seen_generation = stream.generation_past(seen_generation, deadline).await;
    }
    let (_, inbox_rows) = server.mail_list(account_id, &inbox_id).await;
    let flagged = inbox_rows
        .iter()
        .find(|row| row["id"].as_str() == Some(flag_target.as_str()))
        .expect("the flagged message row");
    assert_eq!(
        flagged["isFlagged"], true,
        "the retried keyword change must land"
    );

    server.shutdown().await;
    drop(stalwart);
}

/// A backfill-enabled automation rule applies to mail that was already
/// synced: the rule write creates a durable job, the supervisor's backfill
/// ticks drain it in bounded batches against the real provider, every
/// applied action echoes on /events (advancing the generation the
/// level-triggered way), and the job records completion. The rule triggers
/// only manually, so every tag the account gains is the backfill's work —
/// never the arrival path's.
#[tokio::test(flavor = "multi_thread")]
async fn live_backend_backfills_an_automation_rule_over_seeded_mail() {
    if !integration_enabled() {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = tokio::task::spawn_blocking(StalwartFixture::start)
        .await
        .expect("fixture task");
    let server = LiveServer::spawn().await;
    let account = create_jmap_account(&server, &stalwart).await;
    let account_id = account.id.as_str();

    let mut stream = SseReader::connect(&server).await;
    let handshake = stream.next_data(SSE_FRAME_DEADLINE).await;
    let mut seen_generation = handshake["generation"].as_u64().expect("generation");

    // Every seeded message matches: a from address always carries an '@'.
    // The rule's own preview pins the expected count before the rule exists.
    let condition = serde_json::json!({ "root": {
        "operator": "all",
        "negated": false,
        "nodes": [{
            "type": "condition",
            "field": "fromEmail",
            "operator": "contains",
            "negated": false,
            "value": "@",
        }],
    } });
    let preview = server
        .query(serde_json::json!({ "automationRulePreview": {
            "condition": condition,
            "limit": 1,
        } }))
        .await;
    let expected = preview["data"]["total"].as_i64().expect("preview total");
    assert!(
        expected > 0,
        "the seeded account must hold matching messages before the rule exists"
    );

    server
        .command(&serde_json::json!({
            "id": "cmd-live-backfill-rule-1",
            "command": { "createAutomationRule": { "rule": {
                "id": "rule-live-backfill",
                "name": "Tag the archive",
                "enabled": true,
                "triggers": ["manual"],
                "condition": condition,
                "actions": [{ "kind": "applyTag", "tag": "backfilled-live" }],
                "backfill": true,
            } } }
        }))
        .await;

    // Level-triggered convergence: refetch the tag counts only when the
    // stream reports a generation past the last refetch — the backfill's
    // published echo events are what advance it.
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    let mut tagged_total = 0;
    let mut update_events = 0;
    while tagged_total < expected {
        assert!(
            Instant::now() < deadline,
            "the backfill never tagged all {expected} messages (at {tagged_total} when the deadline passed)"
        );
        let frame = stream.next_data(SSE_FRAME_DEADLINE).await;
        let event = &frame["event"];
        if event["kind"].as_str() == Some("message.updated")
            && event["accountId"].as_str() == Some(account_id)
        {
            update_events += 1;
        }
        let generation = frame["generation"].as_u64().expect("generation");
        if generation <= seen_generation {
            continue;
        }
        seen_generation = generation;
        let tags = server
            .query(serde_json::json!({ "tags": { "accountId": account_id } }))
            .await;
        tagged_total = tags["data"]["rows"]
            .as_array()
            .expect("tag rows")
            .iter()
            .find(|row| row["name"] == "backfilled-live")
            .and_then(|row| row["totalMessages"].as_i64())
            .unwrap_or(0);
    }
    assert_eq!(tagged_total, expected, "every matching message is tagged");
    assert!(
        update_events > 0,
        "applied backfill actions must surface as message.updated events"
    );

    // The durable job records completion. The status write of a trailing
    // empty batch emits no event, so this wait polls the job directly
    // instead of the stream.
    let deadline = Instant::now() + CONVERGENCE_DEADLINE;
    loop {
        let job = server
            .state
            .service
            .automation_backfill_job_for_current_rules(&account.id)
            .expect("job readable")
            .expect("the rule write created a durable job");
        if job.status == AutomationBackfillJobStatus::Completed {
            assert!(job.last_error.is_none(), "a clean drain records no error");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the backfill job never recorded completion (status {:?})",
            job.status
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    server.shutdown().await;
    drop(stalwart);
}
