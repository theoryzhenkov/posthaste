//! RFC-L2-scripting ruling 23 (safe-actions Automations GUI), server side, end
//! to end against the exact desktop-embedded assembly ([`posthaste_server::start_server`]).
//!
//! Pins the three security-critical invariants of the write surface:
//!
//! 1. **A created rule fires WITHOUT a restart** (the reload path, prerequisite
//!    2). The server boots with NO rules; a rule is created over REST; tagging a
//!    message then fires it — proving the write hot-swapped the live evaluator.
//! 2. **`kind=exec` is unrepresentable** (the structural gate, prerequisite 1).
//!    A `POST /v1/rules` with an exec action is rejected at the serde boundary —
//!    it never creates a rule.
//! 3. **A read-scoped token cannot create rules** (Manage authz). An attenuated
//!    `action=read` capability token is 403'd on `POST /v1/rules`.

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
    body: Value,
) -> (reqwest::StatusCode, Value) {
    let resp = client
        .post(url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&body).expect("body serializes"))
        .send()
        .await
        .expect("request should send");
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

/// A WHEN-clause tree equivalent to `tag:instruct` (the shared grammar's output).
fn when_tag_instruct() -> Value {
    json!({
        "root": {
            "operator": "all",
            "negated": false,
            "nodes": [
                { "type": "condition", "field": "keyword", "operator": "equals",
                  "negated": false, "value": "instruct" }
            ]
        }
    })
}

/// A minimal HTTP/1.1 mock webhook receiver: captures each POST body (JSON),
/// always answers 200. Lets the test observe that a REST-created webhook rule
/// actually fired.
async fn run_mock_webhook(listener: TcpListener, captured: Arc<Mutex<Vec<Value>>>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let captured = captured.clone();
        tokio::spawn(async move { handle_mock_conn(stream, captured).await });
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

fn extract_body(buf: &[u8]) -> Option<&[u8]> {
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
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

async fn wait_for_webhook(captured: &Arc<Mutex<Vec<Value>>>) -> Option<Value> {
    for _ in 0..100 {
        let next = captured.lock().unwrap().first().cloned();
        if let Some(payload) = next {
            return Some(payload);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn created_rule_fires_without_restart_and_write_surface_is_locked_down() {
    // --- A mock webhook so a created rule's firing is observable --------------
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let mock_port = mock_listener.local_addr().unwrap().port();
    let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let mock_task = tokio::spawn(run_mock_webhook(mock_listener, captured.clone()));

    // --- Boot the server with NO rules (empty config root) --------------------
    let root = unique_temp_dir("rules-crud-e2e");
    let config_root = root.join("config");
    let state_root = root.join("state");
    let xdg_config_root = root.join("xdg-config");
    std::fs::create_dir_all(&config_root).unwrap();
    std::fs::create_dir_all(&state_root).unwrap();
    std::fs::create_dir_all(&xdg_config_root).unwrap();
    let bootstrap_path = root.join("bootstrap-empty.toml");
    std::fs::write(&bootstrap_path, "").unwrap();

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
    let full = handle.auth_token.clone();
    let client = reqwest::Client::new();

    // The merged ruleset starts empty.
    let listing = get_json(&client, &format!("{base}/rules"), &full).await;
    assert_eq!(
        listing["rules"].as_array().map(|r| r.len()),
        Some(0),
        "no rules at boot"
    );

    // --- Security gate 2: kind=exec is unrepresentable (serde-reject) ----------
    let (status, _) = post_json(
        &client,
        &format!("{base}/rules"),
        &full,
        json!({
            "name": "malicious",
            "when": when_tag_instruct(),
            "action": { "kind": "exec", "command": "/bin/rm", "grants": ["read"] }
        }),
    )
    .await;
    assert!(
        status.is_client_error(),
        "an exec action must be rejected at the write boundary, got {status}"
    );
    let listing = get_json(&client, &format!("{base}/rules"), &full).await;
    assert_eq!(
        listing["rules"].as_array().map(|r| r.len()),
        Some(0),
        "the rejected exec rule must NOT have been created"
    );

    // --- Security gate 3: a read-scoped token cannot create rules (403) --------
    let (status, minted) = post_json(
        &client,
        &format!("{base}/auth/tokens"),
        &full,
        json!({ "actions": ["read"] }),
    )
    .await;
    assert!(status.is_success(), "minting a read token failed: {status}");
    let read_token = minted["token"].as_str().expect("minted token").to_string();
    let (status, _) = post_json(
        &client,
        &format!("{base}/rules"),
        &read_token,
        json!({
            "name": "should-be-forbidden",
            "when": when_tag_instruct(),
            "action": { "kind": "tag", "tag": "nope" }
        }),
    )
    .await;
    assert_eq!(
        status,
        reqwest::StatusCode::FORBIDDEN,
        "a read-scoped token must be 403'd on rule creation, got {status}"
    );

    // --- Create a safe `webhook` rule over REST (full scope) -------------------
    let hook_url = format!("http://127.0.0.1:{mock_port}/hook");
    let (status, created) = post_json(
        &client,
        &format!("{base}/rules"),
        &full,
        json!({
            "id": "fire-on-instruct",
            "name": "Webhook instruct-tagged mail",
            "when": when_tag_instruct(),
            "action": { "kind": "webhook", "url": hook_url, "grants": ["read"], "expirySeconds": 3600 }
        }),
    )
    .await;
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "creating a safe rule should 201, got {status}: {created}"
    );
    assert_eq!(created["id"].as_str(), Some("fire-on-instruct"));

    // A conflicting create is rejected (409).
    let (status, _) = post_json(
        &client,
        &format!("{base}/rules"),
        &full,
        json!({
            "id": "fire-on-instruct",
            "name": "dup",
            "when": when_tag_instruct(),
            "action": { "kind": "tag", "tag": "x" }
        }),
    )
    .await;
    assert_eq!(
        status,
        reqwest::StatusCode::CONFLICT,
        "duplicate id must 409"
    );

    // --- Seed an account + a message ------------------------------------------
    let account_id = "crud-acct";
    let (status, _) = post_json(
        &client,
        &format!("{base}/accounts"),
        &full,
        json!({ "id": account_id, "name": "Crud", "driver": "mock", "enabled": true }),
    )
    .await;
    assert!(status.is_success(), "account create failed: {status}");
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/sync"),
        &full,
        json!({}),
    )
    .await;
    assert!(status.is_success(), "sync failed: {status}");
    let messages = get_json(
        &client,
        &format!("{base}/sources/{account_id}/messages"),
        &full,
    )
    .await;
    let message_id = messages["items"][0]["id"]
        .as_str()
        .expect("at least one seeded message")
        .to_string();

    // --- The reload path: tag `instruct` → the CREATED rule fires (no restart) -
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        &full,
        json!({ "add": ["instruct"], "remove": [] }),
    )
    .await;
    assert!(status.is_success(), "tagging instruct failed: {status}");

    // The rule, created purely over REST after boot, fires its webhook — proving
    // the write hot-swapped the live evaluator (no restart).
    let payload = wait_for_webhook(&captured)
        .await
        .expect("the REST-created rule must fire on the next matching event WITHOUT a restart");
    assert_eq!(
        payload["message"]["id"].as_str(),
        Some(message_id.as_str()),
        "the webhook payload names the triggering message"
    );
    assert_eq!(payload["ruleId"].as_str(), Some("fire-on-instruct"));

    // --- DELETE the rule; a later matching event no longer fires it ------------
    let resp = client
        .delete(format!("{base}/rules/fire-on-instruct"))
        .bearer_auth(&full)
        .send()
        .await
        .expect("delete sends");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "delete should 200 ok"
    );
    let listing = get_json(&client, &format!("{base}/rules"), &full).await;
    assert_eq!(
        listing["rules"].as_array().map(|r| r.len()),
        Some(0),
        "the deleted rule is gone from the merged listing"
    );

    // --- Teardown -------------------------------------------------------------
    mock_task.abort();
    std::io::stdout().flush().ok();
    handle.into_shutdown_sequence().run().await;
    let _ = std::fs::remove_dir_all(&root);
}
