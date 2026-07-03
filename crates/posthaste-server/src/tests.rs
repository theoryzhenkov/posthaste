use std::path::{Path, PathBuf};

use super::*;

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
        if let Some(value) = &self.prior {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[tokio::test]
async fn start_server_does_not_write_daemon_port_file() {
    let root = unique_temp_dir("embedded-startup-test");
    let config_root = root.join("config");
    let state_root = root.join("state");
    let xdg_config_root = root.join("xdg-config");
    std::fs::create_dir_all(&config_root).expect("config root should create");
    std::fs::create_dir_all(&state_root).expect("state root should create");
    let bootstrap_path = root.join("bootstrap-empty.toml");
    std::fs::create_dir_all(&xdg_config_root).expect("xdg config root should create");
    std::fs::write(&bootstrap_path, "").expect("empty bootstrap should write");

    // Pin every daemon setting read from the environment. `start_server()` also
    // calls `dotenv()` in debug builds; dotenv does not override already-set
    // variables, so these guards keep local developer `.env` files from leaking
    // external roots, bootstrap data, or disabled auth into this boundary test.
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

    assert!(handle.require_auth, "bundled perimeter auth should be on");
    // `start_server` stays a pure library entrypoint: it never writes daemon.json
    // itself. Writing the discovery file is the caller's step — `main.rs` for the
    // standalone daemon and the desktop `setup` hook for the embedded app
    // (RFC-L2-scripting §7.7b, via `write_discovery_file`) — so this boundary test
    // still holds.
    assert!(
        !state_root.join("daemon.json").exists(),
        "start_server must not write daemon.json; the caller does"
    );

    handle.join_handle.abort();
    handle
        .runtime_shutdown
        .shutdown()
        .await
        .expect("runtime shutdown should succeed");
    let _ = std::fs::remove_dir_all(&root);
}
