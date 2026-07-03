//! The slice-1 **milestone** (RFC-L2-scripting §7 ruling 7): with the bundled
//! server running, a laptop script works without reading source — the REAL
//! `posthastectl` (bun) auto-discovers the local app, mints a least-privilege
//! token, and `watch --exec`s a script that (a) receives a seeded fact, (b) writes
//! back via `apply` WITH a client-supplied idempotency key, and (c) survives a
//! simulated redelivery without re-executing.
//!
//! **Desktop-embed equivalence.** This test boots [`posthaste_server::start_server`]
//! — the exact bundled assembly the desktop app embeds in-process
//! (`apps/desktop/src/lib.rs` calls the same `start_server` +
//! `write_discovery_file`) and the standalone `posthaste serve` daemon runs
//! (`crates/posthaste-server/src/main.rs`). So "the server the script talks to"
//! here is byte-for-byte the server the desktop app runs; the discovery file is
//! the same one both entrypoints write.
//!
//! Its own integration binary (separate process): `start_server` installs a
//! global tracing subscriber that can be set only once per process. Requires
//! `bun` and `curl` on PATH (the nix dev shell provides both).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use posthaste_http_api_adapter::{write_discovery_file, ServerConfig};
use posthaste_server::start_server;
use serde_json::{json, Value};

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

/// Absolute path to the real `posthastectl` bun entrypoint (`apps/mcp/src/cli.ts`).
fn posthastectl_cli() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/mcp/src/cli.ts")
        .canonicalize()
        .expect("posthastectl cli.ts should exist")
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

/// Kill a spawned `watch` child on drop so a panicking assertion never leaks it.
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A POST helper (reqwest's `json` feature is off in this workspace, so we
/// serialize/parse by hand): returns `(status, json)`.
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
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, value)
}

/// A GET helper returning the parsed JSON body.
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

// Multi-thread runtime: the bundled server runs as spawned tasks on this
// runtime, and the test blocks a worker on `bun` subprocesses (`Command::output`);
// a single-thread runtime would starve the server while blocked and deadlock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn milestone_watch_exec_write_back_survives_redelivery() {
    // --- Boot the bundled server (the desktop-embedded assembly) --------------
    let root = unique_temp_dir("watch-exec-e2e");
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
    assert!(handle.require_auth, "bundled perimeter auth should be on");
    let port = handle.addr.port();
    let base = format!("http://127.0.0.1:{port}/v1");
    let bootstrap_token = handle.auth_token.clone();

    // The discovery rider: the embedded/daemon server writes `daemon.json`;
    // posthastectl auto-discovers it (zero flags). Same call the desktop makes.
    let discovery_path =
        write_discovery_file(handle.addr, &bootstrap_token).expect("discovery file should write");

    let client = reqwest::Client::new();

    // --- Seed a mock account + a fact to write back to ------------------------
    let account_id = "e2e-acct";
    let (status, _) = post_json(
        &client,
        &format!("{base}/accounts"),
        &bootstrap_token,
        None,
        json!({ "id": account_id, "name": "E2E", "driver": "mock", "enabled": true }),
    )
    .await;
    assert!(status.is_success(), "account create failed: {status}");

    // Sync populates the Mock driver's sample messages (the write-back targets).
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/sync"),
        &bootstrap_token,
        None,
        json!({}),
    )
    .await;
    assert!(status.is_success(), "sync failed: {status}");

    // Bootstrap read + snapshot-attach (rider #2): the mail-list read carries the
    // as-of event seq a script would then tail the tap from.
    let messages = get_json(
        &client,
        &format!("{base}/sources/{account_id}/messages"),
        &bootstrap_token,
    )
    .await;
    let as_of_seq = messages.get("asOfSeq");
    assert!(
        as_of_seq.is_some_and(Value::is_u64),
        "the mail-list read must carry asOfSeq (snapshot-attach); got {messages:?}"
    );
    let message_id = messages["items"][0]["id"]
        .as_str()
        .expect("at least one seeded message")
        .to_string();

    // --- The front door: REAL posthastectl auto-discovers + mints a token -----
    let cli = posthastectl_cli();
    let mint = std::process::Command::new("bun")
        .arg(&cli)
        .args([
            "token",
            "mint",
            "--grant",
            "tap:read,apply,read",
            "--expiry",
            "1h",
        ])
        .output()
        .expect("bun posthastectl token mint should run");
    assert!(
        mint.status.success(),
        "token mint failed: {}",
        String::from_utf8_lossy(&mint.stderr)
    );
    let minted_token = String::from_utf8_lossy(&mint.stdout).trim().to_string();
    assert!(!minted_token.is_empty(), "mint printed a token to stdout");
    assert_ne!(
        minted_token, bootstrap_token,
        "the minted token is an attenuation, not the bootstrap token"
    );

    // --- The handler: a script that writes back via apply + idempotency key ---
    let idempotency_key = format!("rule:e2e:{message_id}");
    let marker = root.join("script-ack.json");
    let script_path = root.join("write_back.sh");
    let script = r#"#!/bin/sh
resp=$(curl -sS -X POST \
  "$POSTHASTE_API_URL/sources/$PH_ACCOUNT_ID/commands/messages/$PH_MESSAGE_ID/set-keywords" \
  -H "Authorization: Bearer $POSTHASTE_TOKEN" \
  -H "Idempotency-Key: $IDEMPOTENCY_KEY" \
  -H "content-type: application/json" \
  -d '{"add":["$processed"],"remove":[]}')
printf '%s' "$resp" > "$MARKER"
"#;
    std::fs::write(&script_path, script).unwrap();

    // --- watch --exec: the REAL CLI tails the tap and runs the script ---------
    // The script inherits POSTHASTE_* / IDEMPOTENCY_KEY / MARKER (runCommand
    // merges the watcher's env). Fresh cursor → tails from the live head; watch
    // itself uses the minted (least-privilege) token via POSTHASTE_TOKEN.
    let cursor = root.join("cursor");
    let watch = std::process::Command::new("bun")
        .arg(&cli)
        .args([
            "watch",
            "--all-updates",
            "--exec",
            &format!("sh {}", script_path.display()),
            "--cursor",
            &cursor.display().to_string(),
        ])
        .env("POSTHASTE_API_URL", &base)
        .env("POSTHASTE_TOKEN", &minted_token)
        .env("IDEMPOTENCY_KEY", &idempotency_key)
        .env("MARKER", &marker)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("bun posthastectl watch should spawn");
    let watch_guard = ChildGuard(watch);

    // Let watch connect, then emit a *fresh* live fact each iteration (a unique
    // keyword always changes state → always a message.updated), until the script
    // fires (the marker appears). Robust against subscribe timing.
    tokio::time::sleep(Duration::from_millis(750)).await;
    let mut fired = false;
    for i in 0..40 {
        let (status, _) = post_json(
            &client,
            &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
            &bootstrap_token,
            None,
            json!({ "add": [format!("$probe-{i}")], "remove": [] }),
        )
        .await;
        assert!(status.is_success(), "probe set-keywords failed: {status}");
        tokio::time::sleep(Duration::from_millis(500)).await;
        if marker.exists() && !std::fs::read(&marker).unwrap().is_empty() {
            fired = true;
            break;
        }
    }
    assert!(
        fired,
        "watch --exec did not run the script (no marker after the tap fact)"
    );

    // (a)+(b): the script received the fact and wrote back — the marker holds the
    // first write-back's CommandAck, with a non-empty event set (the keyword was
    // added).
    let ack1: Value =
        serde_json::from_slice(&std::fs::read(&marker).unwrap()).expect("marker is a CommandAck");
    let events1 = ack1["events"].as_array().expect("ack has events");
    assert!(
        !events1.is_empty(),
        "the first keyed write-back applied the change (non-empty events): {ack1:?}"
    );

    // (c) simulated redelivery: re-POST the SAME operation under the SAME key.
    // Idempotent — it re-observes the stored ack (byte-identical events) instead
    // of re-executing. Uses the minted least-privilege token (proves it can write).
    let (status, ack2) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        &minted_token,
        Some(&idempotency_key),
        json!({ "add": ["$processed"], "remove": [] }),
    )
    .await;
    assert!(status.is_success(), "keyed redelivery failed: {status}");
    assert_eq!(
        ack2["events"], ack1["events"],
        "a redelivery under the same key re-observes the first outcome, not a re-execution"
    );

    // Contrast: the SAME operation with NO idempotency key re-executes at the
    // authority, emitting a *fresh* event (new seq) rather than re-observing the
    // stored ack — the very duplicate the key prevents. So its events differ from
    // the first outcome, whereas the keyed redelivery reproduced it exactly.
    let (status, ack3) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/set-keywords"),
        &bootstrap_token,
        None,
        json!({ "add": ["$processed"], "remove": [] }),
    )
    .await;
    assert!(status.is_success());
    let _ = events1;
    assert_ne!(
        ack3["events"], ack1["events"],
        "a keyless re-apply re-executes (fresh event seqs), unlike the deduped redelivery"
    );

    // Same key, DIFFERENT operation → rejected (409 Conflict), the replica-path
    // rule: an idempotency key is bound to its first operation.
    let (status, _) = post_json(
        &client,
        &format!("{base}/sources/{account_id}/commands/messages/{message_id}/destroy"),
        &bootstrap_token,
        Some(&idempotency_key),
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        reqwest::StatusCode::CONFLICT,
        "reusing an idempotency key with a different operation is a 409 Conflict"
    );

    // --- Teardown -------------------------------------------------------------
    drop(watch_guard);
    std::io::stdout().flush().ok();
    handle
        .into_shutdown_sequence()
        .with_discovery_file(discovery_path)
        .run()
        .await;
    let _ = std::fs::remove_dir_all(&root);
}
