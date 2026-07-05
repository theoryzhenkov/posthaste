//! The RFC-L2-scripting §9 **worked example**, end to end: a zero-code
//! declarative rule turns a tagged message into an agent webhook.
//!
//! `rules.toml` carries one enabled rule — *when a message is tagged `instruct`,
//! POST to a webhook, granting `[read, tag]` for one hour* — plus a **disabled**
//! rule that must never fire. The bundled server (the exact desktop-embedded
//! assembly, [`posthaste_server::start_server`]) loads the file, spawns the
//! in-process rule engine, and:
//!
//! 1. tagging a message `instruct` **matches** the rule and fires the webhook;
//! 2. the webhook receives `{event, message, token, idempotencyKey}` where the
//!    token is a **per-invocation, attenuated** macaroon and the key is the
//!    deterministic `rule:<id>:<event_seq>`;
//! 3. the token **applies a reply tag** back through `apply` (the granted `tag`
//!    scope works) — and a **redelivery under the same key is deduped** (no
//!    double-apply);
//! 4. the same token **cannot escalate** — minting (`POST /v1/auth/tokens`) is
//!    refused (the scope wall);
//! 5. the **disabled** rule's effect never appears.
//!
//! Localhost is a first-class webhook target (RFC §7.15): the mock runs on
//! loopback and the delivery path does not block it. Requires no external tools
//! (unlike the `watch --exec` milestone, this is pure Rust).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use posthaste_http_api_adapter::ServerConfig;
use posthaste_server::start_server;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn unique_temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "posthaste-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

struct EnvVarGuard {
    key: &'static str,
    prior: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        let prior = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, prior }
    }
    fn set_value(key: &'static str, value: &str) -> Self {
        let prior = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, prior }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    idempotency_key: Option<&str>,
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let mut req = client
        .post(url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&body).expect("body serializes"));
    if let Some(key) = idempotency_key {
        req = req.header("Idempotency-Key", key);
    }
    let resp = req.send().await.expect("request should send");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get_json(client: &reqwest::Client, url: &str, token: &str) -> Value {
    let text = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .expect("request should send")
        .text()
        .await
        .expect("body text");
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// A minimal HTTP/1.1 mock webhook receiver. Captures each POST body (JSON) into
/// `captured`; always answers `200`.
async fn run_mock_webhook(listener: TcpListener, captured: Arc<Mutex<Vec<Value>>>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let captured = captured.clone();
        tokio::spawn(async move {
            handle_mock_conn(stream, captured).await;
        });
    }
}

async fn handle_mock_conn(mut stream: TcpStream, captured: Arc<Mutex<Vec<Value>>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(body) = extract_body(&buf) {
                    if let Ok(value) = serde_json::from_slice::<Value>(body) {
                        captured.lock().unwrap().push(value);
                    }
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .await;
}

/// Return the request body once the buffer holds the full headers + Content-Length.
fn extract_body(buf: &[u8]) -> Option<&[u8]> {
    let header_end = find_subsequence(buf, b"\r\n\r\n")? + 4;
    let headers = std::str::from_utf8(&buf[..header_end]).ok()?;
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    (buf.len() >= header_end + length).then(|| &buf[header_end..header_end + length])
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// Multi-thread: the bundled server + the mock webhook run as tasks on this
// runtime while the test drives HTTP against both.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn worked_example_instruct_tag_fires_agent_webhook() {
    // --- The mock webhook (loopback: a first-class target, RFC §7.15) ---------
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let mock_port = mock_listener.local_addr().unwrap().port();
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let mock_task = tokio::spawn(run_mock_webhook(mock_listener, captured.clone()));

    // --- rules.toml, written BEFORE the server starts (engine loads at boot) ---
    let root = unique_temp_dir("rules-webhook-e2e");
    let config_root = root.join("config");
    let state_root = root.join("state");
    let xdg_config_root = root.join("xdg-config");
    std::fs::create_dir_all(&config_root).unwrap();
    std::fs::create_dir_all(&state_root).unwrap();
    std::fs::create_dir_all(&xdg_config_root).unwrap();
    let bootstrap_path = root.join("bootstrap-empty.toml");
    std::fs::write(&bootstrap_path, "").unwrap();

    let rules_toml = format!(
        r#"
[[rule]]
id = "instruct-agent"
name = "Send instruct-tagged mail to the agent"
when = "tag:instruct"
enabled = true
action = {{ kind = "webhook", url = "http://127.0.0.1:{mock_port}/hook", grants = ["read", "tag"], expirySeconds = 3600 }}

[[rule]]
id = "disabled-tagger"
name = "This rule is off and must never fire"
when = "tag:instruct"
enabled = false
action = {{ kind = "tag", tag = "$disabled-must-not-appear" }}
"#
    );
    std::fs::write(config_root.join("rules.toml"), rules_toml).unwrap();

    let _config_guard = EnvVarGuard::set("POSTHASTE_CONFIG_ROOT", &config_root);
    let _state_guard = EnvVarGuard::set("POSTHASTE_STATE_ROOT", &state_root);
    let _xdg_config_guard = EnvVarGuard::set("XDG_CONFIG_HOME", &xdg_config_root);
    let _bootstrap_guard = EnvVarGuard::set("POSTHASTE_BOOTSTRAP_PATH", &bootstrap_path);
    let _bind_guard = EnvVarGuard::set_value("POSTHASTE_BIND", "127.0.0.1:0");
    let _cors_guard = EnvVarGuard::set_value("POSTHASTE_CORS_ORIGIN", "http://127.0.0.1:5173");
    let _poll_guard = EnvVarGuard::set_value("POSTHASTE_POLL_INTERVAL", "60");
    let _log_guard = EnvVarGuard::set_value("POSTHASTE_LOG_LEVEL", "warn");
    let _auth_guard = EnvVarGuard::set_value("POSTHASTE_REQUIRE_AUTH", "true");
    let _root_key_guard = EnvVarGuard::set_value(
        "POSTHASTE_MACAROON_ROOT_KEY",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );

    let handle = start_server(ServerConfig {
        bind_address_override: Some("127.0.0.1:0".to_string()),
        ..ServerConfig::default()
    })
    .await;
    let port = handle.addr.port();
    let base = format!("http://127.0.0.1:{port}/v1");
    let full_scope_token = handle.auth_token.clone();
    let client = reqwest::Client::new();

    // --- Seed a mock account + messages ---------------------------------------
    let account_id = "e2e-acct";
    let (status, _) = post_json(
        &client,
        &format!("{base}/accounts"),
        &full_scope_token,
        None,
        json!({ "id": account_id, "name": "E2E", "driver": "mock", "enabled": true }),
    )
    .await;
    assert!(status.is_success(), "account create failed: {status}");
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/sync"),
        &full_scope_token,
        None,
        json!({}),
    )
    .await;
    assert!(status.is_success(), "sync failed: {status}");
    let messages = get_json(
        &client,
        &format!("{base}/sources/{account_id}/messages"),
        &full_scope_token,
    )
    .await;
    let message_id = messages["items"][0]["id"]
        .as_str()
        .expect("at least one seeded message")
        .to_string();

    // --- Trigger: tag the message `instruct` → the rule's WHEN matches ---------
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        &full_scope_token,
        None,
        json!({ "add": ["instruct"], "remove": [] }),
    )
    .await;
    assert!(status.is_success(), "tagging instruct failed: {status}");

    // The engine matches at the AS, mints the token, and POSTs the webhook.
    let payload = wait_for_webhook(&captured).await;

    // (2) The payload carries the event, a token, and the deterministic key.
    let token = payload["token"].as_str().expect("payload carries a token");
    assert!(!token.is_empty());
    let event_seq = payload["event"]["seq"].as_i64().expect("event seq");
    assert_eq!(
        payload["idempotencyKey"].as_str().unwrap(),
        format!("rule:instruct-agent:{event_seq}"),
        "idempotency key is the deterministic f(rule_id, event_seq)"
    );
    assert_eq!(payload["message"]["id"].as_str().unwrap(), message_id);

    // (3) The token applies a reply tag back through `apply` (the `tag` grant).
    let key = payload["idempotencyKey"].as_str().unwrap().to_string();
    let (status, reply_ack) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        token,
        Some(&key),
        json!({ "add": ["$agent-reply"], "remove": [] }),
    )
    .await;
    assert!(
        status.is_success(),
        "the webhook token must be able to tag (its granted scope): {status}"
    );

    // (3, cont.) A redelivery under the SAME key is deduped — no double-apply.
    let (status, reply_ack_again) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        token,
        Some(&key),
        json!({ "add": ["$agent-reply"], "remove": [] }),
    )
    .await;
    assert!(status.is_success());
    assert_eq!(
        reply_ack["events"], reply_ack_again["events"],
        "a redelivery under the same key re-observes the first outcome, not a re-execution"
    );

    // (4) The scope wall: the same token CANNOT escalate — minting is refused.
    let (status, _) = post_json(
        &client,
        &format!("{base}/auth/tokens"),
        token,
        None,
        json!({ "actions": ["read"] }),
    )
    .await;
    assert!(
        status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED,
        "the webhook token must not be able to mint (scope wall): got {status}"
    );

    // The reply tag landed exactly once; the DISABLED rule never fired.
    let after = get_json(
        &client,
        &format!("{base}/sources/{account_id}/messages"),
        &full_scope_token,
    )
    .await;
    let keywords: Vec<String> = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(message_id.as_str()))
        .and_then(|item| item["keywords"].as_array())
        .map(|ks| {
            ks.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        keywords.iter().any(|k| k == "$agent-reply"),
        "the reply tag applied by the webhook token is present: {keywords:?}"
    );
    assert!(
        !keywords.iter().any(|k| k == "$disabled-must-not-appear"),
        "the DISABLED rule must never fire: {keywords:?}"
    );

    // --- Teardown -------------------------------------------------------------
    mock_task.abort();
    std::io::stdout().flush().ok();
    handle.into_shutdown_sequence().run().await;
    let _ = std::fs::remove_dir_all(&root);
}

/// Poll the mock's capture buffer for the first webhook delivery (the engine
/// subscribes/matches/mints asynchronously). Fails the test on timeout.
async fn wait_for_webhook(captured: &Arc<Mutex<Vec<Value>>>) -> Value {
    for _ in 0..100 {
        let next = captured.lock().unwrap().first().cloned();
        if let Some(payload) = next {
            return payload;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the rule webhook never fired (no delivery captured within 10s)");
}
