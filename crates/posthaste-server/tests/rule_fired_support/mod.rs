//! Shared harness for the RFC-L2-scripting S3 durable-tap tests
//! (`rule_fired_durable_replay.rs` / `rule_fired_gap_frame.rs`).
//!
//! Split into two top-level test files (not one, despite sharing this module)
//! because `posthaste_server::start_server` calls the process-global
//! `tracing_subscriber::...::init()` exactly once per process — two
//! `#[tokio::test]` functions that both start a bundled server in the SAME
//! binary panic on the second `init()`. Each file under `tests/` compiles to
//! its own binary/process, so splitting is the fix; this module (a
//! subdirectory, not a top-level file) is not itself treated as a test binary.

// Each of the two test binaries only exercises a subset of this shared harness
// (e.g. only the gap-frame test deletes an account) — matches the convention
// in `tests/support/mod.rs`.
#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use posthaste_http_api_adapter::{ServerConfig, ServerHandle};
use posthaste_server::start_server;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

pub const EMIT_RULE_TOML: &str = r#"
[[rule]]
id = "emit-on-instruct"
name = "Emit rule.fired when tagged instruct"
when = "tag:instruct"
enabled = true
action = { kind = "emit" }
"#;

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

async fn delete_ok(client: &reqwest::Client, url: &str, token: &str) {
    let status = client
        .delete(url)
        .bearer_auth(token)
        .send()
        .await
        .expect("request should send")
        .status();
    assert!(status.is_success(), "delete failed: {status}");
}

/// Open `GET path` as an SSE stream and accumulate raw text until every needle
/// in `needles` has appeared (or `timeout` elapses). Mirrors how a real
/// consumer (posthastectl) reads the stream incrementally rather than waiting
/// for EOF (the stream never ends on its own — the live tail keeps it open).
async fn read_sse_until(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    needles: &[&str],
    timeout: Duration,
) -> String {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .expect("sse request should send");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "sse open should succeed"
    );
    let mut stream = response.bytes_stream();
    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if needles.iter().all(|needle| collected.contains(needle)) {
            return collected;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for {needles:?} in the SSE stream; got so far:\n{collected}");
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => collected.push_str(&String::from_utf8_lossy(&chunk)),
            Ok(Some(Err(error))) => panic!("sse stream error: {error}"),
            Ok(None) => panic!("sse stream ended before {needles:?} appeared:\n{collected}"),
            Err(_) => {
                panic!(
                    "timed out waiting for {needles:?} in the SSE stream; got so far:\n{collected}"
                )
            }
        }
    }
}

/// Extract the `seq` of the first `data:` frame whose JSON payload matches
/// `topic`. Panics if none is found — callers only call this after
/// `read_sse_until` already proved the substring is present.
pub fn seq_of_topic_frame(sse_text: &str, topic: &str) -> i64 {
    for line in sse_text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        if value["topic"].as_str() == Some(topic) {
            return value["seq"].as_i64().expect("event carries a seq");
        }
    }
    panic!("no data frame with topic {topic} found in:\n{sse_text}");
}

pub struct Harness {
    client: reqwest::Client,
    base: String,
    full_scope_token: String,
    root: PathBuf,
    _config_guard: EnvVarGuard,
    _state_guard: EnvVarGuard,
    _xdg_config_guard: EnvVarGuard,
    _bootstrap_guard: EnvVarGuard,
    _bind_guard: EnvVarGuard,
    _cors_guard: EnvVarGuard,
    _poll_guard: EnvVarGuard,
    _log_guard: EnvVarGuard,
    _auth_guard: EnvVarGuard,
    _root_key_guard: EnvVarGuard,
    handle: Option<ServerHandle>,
}

impl Harness {
    pub async fn start(label: &str, rules_toml: &str) -> Self {
        let root = unique_temp_dir(label);
        let config_root = root.join("config");
        let state_root = root.join("state");
        let xdg_config_root = root.join("xdg-config");
        std::fs::create_dir_all(&config_root).unwrap();
        std::fs::create_dir_all(&state_root).unwrap();
        std::fs::create_dir_all(&xdg_config_root).unwrap();
        let bootstrap_path = root.join("bootstrap-empty.toml");
        std::fs::write(&bootstrap_path, "").unwrap();
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

        Self {
            client: reqwest::Client::new(),
            base,
            full_scope_token,
            root,
            _config_guard,
            _state_guard,
            _xdg_config_guard,
            _bootstrap_guard,
            _bind_guard,
            _cors_guard,
            _poll_guard,
            _log_guard,
            _auth_guard,
            _root_key_guard,
            handle: Some(handle),
        }
    }

    /// Create + sync a mock account, returning its first seeded message id.
    pub async fn seed_account(&self, account_id: &str) -> String {
        let (status, _) = post_json(
            &self.client,
            &format!("{}/accounts", self.base),
            &self.full_scope_token,
            json!({ "id": account_id, "name": account_id, "driver": "mock", "enabled": true }),
        )
        .await;
        assert!(status.is_success(), "account create failed: {status}");
        let (status, _) = post_json(
            &self.client,
            &format!("{}/sources/{account_id}/commands/sync", self.base),
            &self.full_scope_token,
            json!({}),
        )
        .await;
        assert!(status.is_success(), "sync failed: {status}");
        let messages = get_json(
            &self.client,
            &format!("{}/sources/{account_id}/messages", self.base),
            &self.full_scope_token,
        )
        .await;
        messages["items"][0]["id"]
            .as_str()
            .expect("at least one seeded message")
            .to_string()
    }

    pub async fn tag_instruct(&self, account_id: &str, message_id: &str) {
        let (status, _) = post_json(
            &self.client,
            &format!(
                "{}/sources/{account_id}/commands/messages/{message_id}/set-keywords",
                self.base
            ),
            &self.full_scope_token,
            json!({ "add": ["instruct"], "remove": [] }),
        )
        .await;
        assert!(status.is_success(), "tagging instruct failed: {status}");
    }

    pub async fn delete_account(&self, account_id: &str) {
        delete_ok(
            &self.client,
            &format!("{}/accounts/{account_id}", self.base),
            &self.full_scope_token,
        )
        .await;
    }

    pub async fn events_containing(&self, after_seq: u64, needles: &[&str]) -> String {
        read_sse_until(
            &self.client,
            &format!("{}/events?afterSeq={after_seq}", self.base),
            &self.full_scope_token,
            needles,
            Duration::from_secs(10),
        )
        .await
    }

    pub async fn shutdown(mut self) {
        std::io::stdout().flush().ok();
        self.handle
            .take()
            .expect("handle present")
            .into_shutdown_sequence()
            .run()
            .await;
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
