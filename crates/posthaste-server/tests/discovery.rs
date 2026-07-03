//! The discovery rider (RFC-L2-scripting §7 ruling 7b): a running server writes a
//! well-known `daemon.json` into its state dir; a same-machine client reads it and
//! connects with no flags. This proves the write → read → connect loop end to end
//! against a real bound server, and that a clean teardown removes the file.
//!
//! Its own integration binary (separate process): `start_server` installs a
//! global tracing subscriber that can be set only once per process.

use std::path::{Path, PathBuf};

use posthaste_http_api_adapter::{write_discovery_file, ServerConfig};
use posthaste_server::start_server;

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

/// Start the bundled server, write the discovery file, then (as a client would)
/// read the port + token back out of it and make an authenticated request —
/// proving zero-flag auto-discovery works. Finally, run the ordered teardown with
/// the discovery file wired in and assert it is removed.
#[tokio::test]
async fn discovery_file_is_written_read_and_connects_then_removed_on_shutdown() {
    let root = unique_temp_dir("discovery-test");
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
    assert!(handle.require_auth, "bundled perimeter auth should be on");

    // Write the discovery file (the caller's step — here standing in for main.rs /
    // the desktop setup hook).
    let discovery_path =
        write_discovery_file(handle.addr, &handle.auth_token).expect("discovery file should write");
    assert_eq!(discovery_path, state_root.join("daemon.json"));

    // Read + parse it (as posthastectl's `resolveConnection` does): a versioned
    // `{ port, url, token }`.
    let raw = std::fs::read_to_string(&discovery_path).expect("discovery file should read");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("discovery file is JSON");
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["port"], handle.addr.port());
    assert_eq!(
        parsed["url"],
        format!("http://127.0.0.1:{}/v1", handle.addr.port())
    );
    assert_eq!(parsed["token"], handle.auth_token);

    // The credential is owner-only on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&discovery_path)
            .expect("discovery file metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "daemon.json must be 0600");
    }

    // Connect using ONLY what the file provided — the discovery contract.
    let discovered_port = parsed["port"].as_u64().expect("port is a number");
    let discovered_token = parsed["token"].as_str().expect("token is a string");
    let url = format!("http://127.0.0.1:{discovered_port}/v1/openapi.json");
    {
        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth(discovered_token)
            .send()
            .await
            .expect("the discovered server returns an HTTP response");
        assert!(
            response.status().is_success(),
            "an authenticated request via the discovered port/token succeeds (status {})",
            response.status()
        );
    }

    // Clean teardown removes the discovery file (final M20 step).
    handle
        .into_shutdown_sequence()
        .with_discovery_file(discovery_path.clone())
        .run()
        .await;
    assert!(
        !discovery_path.exists(),
        "the discovery file is removed on clean shutdown"
    );

    let _ = std::fs::remove_dir_all(&root);
}
