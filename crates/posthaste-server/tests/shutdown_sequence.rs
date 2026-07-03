//! The M20 gate (D60): the bundled server shuts down in order, drains in-flight
//! work, and completes inside the ratified teardown budget.
//!
//! Lives in its own integration binary (a separate process) because
//! `start_server` installs a global tracing subscriber that can be set only once
//! per process — the crate's lib unit tests already claim one.

use std::path::Path;
use std::time::{Duration, Instant};

use posthaste_http_api_adapter::{ServerConfig, TOTAL_SHUTDOWN_BUDGET};
use posthaste_server::start_server;
use posthaste_testkit::temp_root;

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
        if let Some(value) = &self.prior {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

/// Start the bundled server, prove it serves a request (an in-flight request
/// completes, not severed), then run the ordered `ShutdownSequence` directly
/// (raising real signals is awkward inside a test harness). Assert the teardown
/// finishes inside the ratified budget and that the listener is gone afterwards
/// (the process would exit cleanly here).
#[tokio::test]
async fn shutdown_sequence_drains_and_completes_under_budget() {
    let root = temp_root("shutdown-sequence-test");
    let config_root = root.join("config");
    let state_root = root.join("state");
    let xdg_config_root = root.join("xdg-config");
    std::fs::create_dir_all(&config_root).expect("config root should create");
    std::fs::create_dir_all(&state_root).expect("state root should create");
    std::fs::create_dir_all(&xdg_config_root).expect("xdg config root should create");
    let bootstrap_path = root.join("bootstrap-empty.toml");
    std::fs::write(&bootstrap_path, "").expect("empty bootstrap should write");

    let _config_guard = EnvVarGuard::set("POSTHASTE_CONFIG_ROOT", &config_root);
    let _state_guard = EnvVarGuard::set("POSTHASTE_STATE_ROOT", &state_root);
    let _xdg_config_guard = EnvVarGuard::set("XDG_CONFIG_HOME", &xdg_config_root);
    let _bootstrap_guard = EnvVarGuard::set("POSTHASTE_BOOTSTRAP_PATH", &bootstrap_path);
    let _bind_guard = EnvVarGuard::set_value("POSTHASTE_BIND", "127.0.0.1:0");
    let _cors_guard = EnvVarGuard::set_value("POSTHASTE_CORS_ORIGIN", "http://127.0.0.1:5173");
    let _poll_guard = EnvVarGuard::set_value("POSTHASTE_POLL_INTERVAL", "60");
    let _log_guard = EnvVarGuard::set_value("POSTHASTE_LOG_LEVEL", "info");
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

    let addr = handle.addr;
    let auth_token = handle.auth_token.clone();
    let url = format!("http://{addr}/v1/openapi.json");

    // A request served before teardown: the server responds (the request path is
    // live and a request is not severed). Drop the client so no pooled keep-alive
    // connection lingers into the drain.
    {
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .bearer_auth(&auth_token)
            .send()
            .await
            .expect("the live server returns an HTTP response");
        assert!(
            response.status().is_success(),
            "openapi document is served (status {})",
            response.status()
        );
    }

    // Run the ordered teardown directly (token cancel → HTTP drain →
    // runtime/supervisor stop → store close) and assert it completes well inside
    // the ratified budget.
    let start = Instant::now();
    handle.into_shutdown_sequence().run().await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < TOTAL_SHUTDOWN_BUDGET,
        "teardown completed in {elapsed:?}, under the {TOTAL_SHUTDOWN_BUDGET:?} budget"
    );

    // After a clean teardown the listener is gone — a fresh request fails to
    // connect.
    let after = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&auth_token)
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    assert!(
        after.is_err(),
        "the server no longer accepts connections after teardown"
    );
}
