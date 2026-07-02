//! Resolved daemon settings: read `app.toml` `[daemon]`/`[link]`/`[tls]`/
//! `[logging]` sections and apply `POSTHASTE_*` environment overrides.
//!
//! This module is the single owner of the daemon config resolution story. It
//! lives in the config crate, so it may use `AppToml`/schema internals directly
//! — the `AppToml` value returned by [`TomlConfigRepository::read_app_toml`]
//! never crosses the crate boundary.
//!
//! @spec docs/L1-accounts#apptoml

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use posthaste_domain_service::ConfigError;

use crate::TomlConfigRepository;

/// Runtime settings for the daemon process, read from `app.toml` `[daemon]`
/// section with environment variable overrides.
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug)]
pub struct DaemonSettings {
    pub bind_address: String,
    pub cors_origin: String,
    pub poll_interval_seconds: u64,
    pub log_level: String,
    /// When `true`, the `/v1` API enforces bearer-token + Origin/Host auth.
    /// Defaults to `true` (perimeter on); an explicit `app.toml`/env override
    /// can disable it.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
    /// Authority server role: when `true`, mount the runtime↔authority-server `link_router` so a
    /// remote runtime can drive this authority server over HTTP. Default `false` (the
    /// bundled single-process deployment never exposes the link).
    ///
    /// @spec docs/replication/L1#10-deployment-topology
    pub link_serve: bool,
    /// Connect role: this near node's bearer token, presented to the remote
    /// authority server (single token — the near node is one runtime).
    pub link_token: Option<String>,
    /// Serve role: the runtimes permitted to connect, as `token → runtime_id`
    /// (X ≥ 1). Required under `link_serve` + `require_auth`; serving without
    /// it is refused.
    pub link_runtimes: Option<HashMap<String, String>>,
    /// Runtime role: when set, this process connects to a remote authority server at this
    /// base URL over the link instead of using the in-process one.
    pub link_authority_server_url: Option<String>,
    /// Optional in-daemon TLS (`[tls]` cert+key paths). Present ⇒ serve HTTPS
    /// over the bound address; absent = plaintext loopback.
    pub tls: Option<TlsConfig>,
    /// Extra hosts admitted by the `Host`-header DNS-rebinding guard (remote
    /// clients over a hostname; a wildcard bind admits no external host).
    pub allowed_hosts: Vec<String>,
}

/// Resolved `[tls]` config: filesystem paths to a PEM cert chain + private key.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Open the config repository at `config_root` and read the daemon settings.
/// A convenience for hosts that build the daemon directly from a config root
/// without constructing a [`TomlConfigRepository`] themselves.
pub fn load_daemon_settings(config_root: &Path) -> DaemonSettings {
    let repo = TomlConfigRepository::open(config_root).expect("failed to open config directory");
    read_daemon_settings(&repo).expect("failed to read runtime settings")
}

/// Read daemon settings from `app.toml` `[daemon]` section, with env var
/// overrides for bind address, CORS origin, and poll interval.
///
/// @spec docs/L1-accounts#apptoml
pub fn read_daemon_settings(
    config_repo: &TomlConfigRepository,
) -> Result<DaemonSettings, ConfigError> {
    let app_toml = config_repo.read_app_toml()?;

    // Also check env vars that may override
    let bind = std::env::var("POSTHASTE_BIND")
        .ok()
        .or(app_toml.daemon.bind)
        .unwrap_or_else(|| "127.0.0.1:3001".to_string());

    let cors_origin = std::env::var("POSTHASTE_CORS_ORIGIN")
        .ok()
        .or(app_toml.daemon.cors_origin)
        .unwrap_or_else(|| "http://localhost:5173".to_string());

    let poll_interval_seconds = std::env::var("POSTHASTE_POLL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .or(app_toml.daemon.poll_interval_seconds)
        .unwrap_or(60);

    let log_level = std::env::var("POSTHASTE_LOG_LEVEL")
        .ok()
        .or(app_toml.logging.level)
        .unwrap_or_else(|| "info".to_string());

    // Perimeter auth defaults ON. Explicit config/env wins (an app.toml
    // `[daemon] require_auth` or `POSTHASTE_REQUIRE_AUTH=false` still disables
    // it); absence resolves to enabled.
    //
    // @spec docs/eph/DESIGN-L1-trust-model
    let require_auth = std::env::var("POSTHASTE_REQUIRE_AUTH")
        .ok()
        .and_then(|v| parse_bool_flag(&v))
        .or(app_toml.daemon.require_auth)
        .unwrap_or(true);

    // Runtime↔authority server link roles (default: in-process, not served). Env wins
    // over `[link]` so a split can be dogfooded without editing config.
    let link_serve = std::env::var("POSTHASTE_LINK_SERVE")
        .ok()
        .and_then(|v| parse_bool_flag(&v))
        .or(app_toml.link.serve)
        .unwrap_or(false);
    let link_token = std::env::var("POSTHASTE_LINK_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .or(app_toml.link.token);
    let link_authority_server_url = std::env::var("POSTHASTE_LINK_AUTHORITY_SERVER_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .or(app_toml.link.authority_server_url);
    // Serve role: the `token → runtime_id` map (TOML only — a map is awkward in
    // env). X runtimes, X ≥ 1.
    let link_runtimes = app_toml.link.runtimes.clone();

    // Optional in-daemon TLS. Both cert+key must be present together; a partial
    // [tls] table is a validation error (fail closed, not silent plaintext).
    let tls = app_toml
        .tls
        .as_ref()
        .map(|t| {
            let cert_path = t.cert.clone().ok_or_else(|| {
                ConfigError::Validation("[tls] cert is required when [tls] is present".into())
            })?;
            let key_path = t.key.clone().ok_or_else(|| {
                ConfigError::Validation("[tls] key is required when [tls] is present".into())
            })?;
            Ok::<_, ConfigError>(TlsConfig {
                cert_path,
                key_path,
            })
        })
        .transpose()?;
    let allowed_hosts = app_toml.daemon.allowed_hosts;

    Ok(DaemonSettings {
        bind_address: bind,
        cors_origin,
        poll_interval_seconds,
        log_level,
        require_auth,
        link_serve,
        link_token,
        link_runtimes,
        link_authority_server_url,
        tls,
        allowed_hosts,
    })
}

// -- Helpers --

fn parse_bool_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
