//! Client-owned connection state for the desktop shell.
//!
//! Phase B of the deployment-modes design. The desktop client owns a small
//! amount of state that is *distinct from* the daemon's `STATE_ROOT`/`CONFIG_ROOT`:
//! the connection-profile store (`connections.json`) and the per-profile remote
//! tokens (in the OS keyring). These Tauri commands expose that state to the web
//! frontend's `ClientStore` desktop backend.
//!
//! Storage locations (client-owned, never the daemon roots):
//!   - `connections.json`: `<app_config_dir>/client/connections.json`, where
//!     `app_config_dir` is Tauri's per-app config directory (e.g.
//!     `~/.config/com.posthaste.desktop/` on Linux). This is deliberately the
//!     Tauri app-config dir, NOT `POSTHASTE_CONFIG_ROOT`/`POSTHASTE_STATE_ROOT`.
//!   - per-profile remote tokens: OS keyring, service `posthaste-client`,
//!     account = the profile id. Never written to `connections.json`.
//!
//! `daemon.json` (read-only here) is still owned by the daemon and lives under
//! its `STATE_ROOT`; the `local-daemon` profile mode reads it to discover a
//! locally-running daemon, mirroring `apps/mcp/src/client.ts`.

use std::path::PathBuf;

use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, Runtime};

/// Keyring service name for client-held per-profile remote tokens. Kept
/// distinct from the daemon's `posthaste` service so client and daemon secrets
/// never collide.
pub(crate) const CLIENT_KEYRING_SERVICE: &str = "posthaste-client";

/// File name of the client connection-profile store.
const CONNECTIONS_FILE: &str = "connections.json";

/// Sub-directory under the app-config dir that holds client-owned state.
const CLIENT_SUBDIR: &str = "client";

/// The daemon port-file, mirrored from `apps/mcp/src/client.ts`. The daemon
/// writes `{ version, port, token }`; unknown fields are tolerated.
#[derive(Debug, Deserialize)]
struct DaemonPortFile {
    #[allow(dead_code)]
    version: Option<u32>,
    port: u16,
    token: String,
}

/// The resolved local-daemon discovery result handed back to the frontend.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDaemon {
    pub port: u16,
    pub token: String,
}

fn client_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_config_dir()
        .map_err(|err| format!("cannot resolve app config dir: {err}"))?;
    Ok(base.join(CLIENT_SUBDIR))
}

/// Resolve the daemon `STATE_ROOT`, mirroring `posthaste-server`'s config and
/// `apps/mcp/src/client.ts`: `POSTHASTE_STATE_ROOT`, else `$XDG_DATA_HOME/posthaste`,
/// else `~/.local/share/posthaste`. The same XDG fallback applies on every
/// platform (no macOS `Application Support` special-case).
fn daemon_state_root() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("POSTHASTE_STATE_ROOT") {
        if !explicit.is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("posthaste"));
        }
    }
    dirs::home_dir().map(|home| home.join(".local").join("share").join("posthaste"))
}

/// Request a local-database repair on the next launch.
///
/// Writes the marker file the embedded server checks on open (mirrors
/// `posthaste_store::REPAIR_MARKER_FILE`). On restart the corrupt database is
/// quarantined and rebuilt from config; accounts and secrets are unaffected.
/// The frontend calls this and then relaunches the app.
#[tauri::command]
pub fn request_database_repair() -> Result<(), String> {
    let Some(state_root) = daemon_state_root() else {
        return Err("cannot resolve the Posthaste data directory".to_string());
    };
    std::fs::create_dir_all(&state_root)
        .map_err(|err| format!("cannot create {}: {err}", state_root.display()))?;
    let marker = state_root.join(".repair-requested");
    std::fs::write(&marker, b"")
        .map_err(|err| format!("cannot write {}: {err}", marker.display()))?;
    Ok(())
}

/// Marker that requests a full factory reset on the next launch.
const FACTORY_RESET_MARKER: &str = ".factory-reset-requested";

/// Resolve the daemon `CONFIG_ROOT` (app.toml + sources + smart-mailboxes),
/// mirroring `posthaste-server`: `POSTHASTE_CONFIG_ROOT`, else
/// `$XDG_CONFIG_HOME/posthaste`, else `~/.config/posthaste`.
fn daemon_config_root() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("POSTHASTE_CONFIG_ROOT") {
        if !explicit.is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("posthaste"));
        }
    }
    dirs::home_dir().map(|home| home.join(".config").join("posthaste"))
}

/// Wipe everything a factory reset owns on the daemon side: the state root
/// (mail.sqlite + cache + daemon.json), the config root (app.toml + sources +
/// smart-mailboxes), and the client connection store. Each root is removed then
/// recreated empty; missing paths are a no-op (idempotent). Keyring secrets are
/// left as harmless orphans (re-adding an account overwrites them).
fn wipe_factory_reset_targets(
    state_root: &std::path::Path,
    config_root: Option<&std::path::Path>,
    client_connections: Option<&std::path::Path>,
) -> std::io::Result<()> {
    for root in std::iter::once(state_root).chain(config_root) {
        if root.exists() {
            std::fs::remove_dir_all(root)?;
        }
        std::fs::create_dir_all(root)?;
    }
    if let Some(path) = client_connections {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Request a full factory reset on the next launch.
///
/// Writes the marker [`consume_factory_reset_marker`] acts on at startup (before
/// the embedded server opens anything). The frontend also clears the client
/// replica + local UI state, then relaunches. Unlike repair this also removes
/// accounts + config; the user starts from a clean install.
#[tauri::command]
pub fn request_factory_reset() -> Result<(), String> {
    let Some(state_root) = daemon_state_root() else {
        return Err("cannot resolve the Posthaste data directory".to_string());
    };
    std::fs::create_dir_all(&state_root)
        .map_err(|err| format!("cannot create {}: {err}", state_root.display()))?;
    let marker = state_root.join(FACTORY_RESET_MARKER);
    std::fs::write(&marker, b"")
        .map_err(|err| format!("cannot write {}: {err}", marker.display()))?;
    Ok(())
}

/// Sanitized runtime info for the "Copy diagnostics" support bundle: the app
/// version, OS/arch, and the daemon log directory path. No secrets, message
/// bodies, or account data cross this boundary — account status is gathered
/// renderer-side from the accounts query.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInfo {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub log_dir_path: Option<String>,
}

/// Return sanitized runtime info for the diagnostics bundle.
#[tauri::command]
pub fn get_diagnostics_info<R: Runtime>(app: AppHandle<R>) -> Result<DiagnosticsInfo, String> {
    let log_dir_path =
        daemon_state_root().map(|root| root.join("logs").to_string_lossy().into_owned());
    Ok(DiagnosticsInfo {
        app_version: app.package_info().version.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        log_dir_path,
    })
}

/// Open the daemon log directory in the OS file manager so the user can attach
/// the JSONL logs to a bug report. Best-effort: a missing directory is created;
/// an open failure returns an error string the renderer toasts.
#[tauri::command]
pub fn reveal_log_folder() -> Result<(), String> {
    let Some(state_root) = daemon_state_root() else {
        return Err("cannot resolve the Posthaste data directory".to_string());
    };
    let log_dir = state_root.join("logs");
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)
            .map_err(|err| format!("cannot create {}: {err}", log_dir.display()))?;
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(&log_dir)
        .spawn()
        .map_err(|err| format!("cannot open {}: {err}", log_dir.display()))?;
    Ok(())
}

/// If a factory reset was requested, perform it before the embedded server
/// starts (so nothing holds the files open) and return `true`. Best-effort: a
/// partial wipe must not block launch. Call once at startup, before the server.
pub(crate) fn consume_factory_reset_marker<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state_root) = daemon_state_root() else {
        return false;
    };
    if !state_root.join(FACTORY_RESET_MARKER).exists() {
        return false;
    }
    let config_root = daemon_config_root();
    let connections = client_dir(app).ok().map(|dir| dir.join(CONNECTIONS_FILE));
    let _ = wipe_factory_reset_targets(&state_root, config_root.as_deref(), connections.as_deref());
    true
}

/// Read the connection-profile store as a raw JSON string, or `None` when the
/// file does not exist yet (a fresh install). The frontend owns parsing so the
/// version-tolerant schema lives in one place (TypeScript).
#[tauri::command]
pub fn client_connections_read<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    let path = client_dir(&app)?.join(CONNECTIONS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("cannot read {}: {err}", path.display())),
    }
}

fn is_allowed_connections_file_key(key: &str) -> bool {
    matches!(key, "version" | "activeProfileId" | "profiles")
}

fn is_allowed_connection_profile_key(key: &str) -> bool {
    matches!(
        key,
        "id" | "name" | "baseUrl" | "hostHeader" | "mode" | "tokenRef"
    )
}

fn percent_decode_ascii_once(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded.push(byte as char);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[index] as char);
        index += 1;
    }
    decoded
}

fn percent_decode_ascii(value: &str) -> (String, bool) {
    let mut current = value.to_string();
    for _ in 0..8 {
        let decoded = percent_decode_ascii_once(&current);
        if decoded == current {
            return (current, true);
        }
        current = decoded;
    }
    (current, false)
}

fn contains_secret_marker(value: &str) -> bool {
    let (decoded, stable) = percent_decode_ascii(value);
    if !stable {
        return true;
    }
    let normalized = decoded
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "authheader",
        "bearer",
        "apikey",
        "privatekey",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn is_inline_secret_field(key: &str) -> bool {
    key != "tokenRef" && contains_secret_marker(key)
}

fn object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a Value>, String> {
    Ok(object.get(field))
}

fn string_field(object: &Map<String, Value>, field: &str) -> Result<Option<String>, String> {
    object
        .get(field)
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("connection profile field {field} must be a string"))
        })
        .transpose()
}

fn contains_url_path_secret_marker(value: &str) -> bool {
    let (decoded, stable) = percent_decode_ascii(value);
    if !stable {
        return true;
    }
    decoded
        .split(['/', ';'])
        .map(|segment| {
            segment
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .any(|segment| {
            segment == "token"
                || segment == "secret"
                || segment.contains("token")
                || segment.contains("secret")
                || segment.contains("authorization")
                || segment.contains("authheader")
                || segment.contains("bearer")
                || segment.contains("apikey")
                || segment.contains("privatekey")
                || segment.contains("password")
                || segment.contains("credential")
        })
}

fn validate_profile_base_url(value: &str) -> Result<(), String> {
    let parsed = url::Url::parse(value).map_err(|err| format!("invalid profile baseUrl: {err}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("profile baseUrl must use http or https".to_string());
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || contains_url_path_secret_marker(parsed.path())
    {
        return Err(
            "profile baseUrl must not contain credentials, query, fragment, or secret path markers"
                .to_string(),
        );
    }
    Ok(())
}

fn validate_host_header(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("connection profile hostHeader is unsafe".to_string());
    }
    let parsed = url::Url::parse(&format!("http://{value}"))
        .map_err(|err| format!("invalid connection profile hostHeader: {err}"))?;
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("connection profile hostHeader must be host[:port]".to_string());
    }
    Ok(())
}

fn validate_connection_profile(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "connection profiles must be objects".to_string())?;
    for key in object.keys() {
        if !is_allowed_connection_profile_key(key) || is_inline_secret_field(key) {
            return Err(format!("connection profile contains unsafe field {key}"));
        }
    }
    if string_field(object, "id")?.is_none() {
        return Err("connection profile missing required field id".to_string());
    }
    if string_field(object, "name")?.is_none() {
        return Err("connection profile missing required field name".to_string());
    }
    let mode = string_field(object, "mode")?
        .ok_or_else(|| "connection profile missing required field mode".to_string())?;
    if !matches!(mode.as_str(), "embedded" | "local-daemon" | "remote") {
        return Err(format!("connection profile mode {mode:?} is unsupported"));
    }
    if let Some(host_header) = string_field(object, "hostHeader")? {
        validate_host_header(&host_header)?;
    }
    let _ = string_field(object, "tokenRef")?;
    if let Some(base_url) = string_field(object, "baseUrl")? {
        validate_profile_base_url(&base_url)?;
    }
    Ok(())
}

fn validate_connections_value(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "connections.json must be an object".to_string())?;
    for key in object.keys() {
        if !is_allowed_connections_file_key(key) || is_inline_secret_field(key) {
            return Err(format!("connection store contains unsafe field {key}"));
        }
    }
    if object.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("connection store version must be 1".to_string());
    }
    if !matches!(
        object_field(object, "activeProfileId")?,
        Some(Value::String(_)) | Some(Value::Null)
    ) {
        return Err("connection store activeProfileId must be a string or null".to_string());
    }
    let profiles = object
        .get("profiles")
        .and_then(Value::as_array)
        .ok_or_else(|| "connection store profiles must be an array".to_string())?;
    for profile in profiles {
        validate_connection_profile(profile)?;
    }
    Ok(())
}

pub(crate) fn canonical_connections_json(contents: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(contents)
        .map_err(|err| format!("connections.json is not valid JSON: {err}"))?;
    validate_connections_value(&value)?;
    serde_json::to_string(&value)
        .map_err(|err| format!("connections.json could not be serialized: {err}"))
}

#[cfg(test)]
pub(crate) fn validate_connections_json(contents: &str) -> Result<(), String> {
    canonical_connections_json(contents).map(|_| ())
}

/// Persist the connection-profile store. The frontend serializes the
/// `ConnectionsFile` (which holds NO secrets — tokens live in the keyring).
#[tauri::command]
pub fn client_connections_write<R: Runtime>(
    app: AppHandle<R>,
    contents: String,
) -> Result<(), String> {
    let safe_contents = canonical_connections_json(&contents)?;
    let dir = client_dir(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let path = dir.join(CONNECTIONS_FILE);
    std::fs::write(&path, safe_contents)
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn client_token_entry(profile_id: &str) -> Result<Entry, String> {
    Entry::new(CLIENT_KEYRING_SERVICE, profile_id)
        .map_err(|err| format!("keyring unavailable: {err}"))
}

/// Read a per-profile remote token from the OS keyring, or `None` if absent.
#[tauri::command]
pub fn client_token_get(profile_id: String) -> Result<Option<String>, String> {
    match client_token_entry(&profile_id)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(err) => Err(format!("keyring read failed: {err}")),
    }
}

/// Store (or replace) a per-profile remote token in the OS keyring. Tokens are
/// NEVER written to `connections.json`.
#[tauri::command]
pub fn client_token_set(profile_id: String, token: String) -> Result<(), String> {
    client_token_entry(&profile_id)?
        .set_password(&token)
        .map_err(|err| format!("keyring write failed: {err}"))
}

/// Delete a per-profile remote token from the OS keyring. Absent entries are a
/// no-op so profile removal is idempotent.
#[tauri::command]
pub fn client_token_delete(profile_id: String) -> Result<(), String> {
    match client_token_entry(&profile_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(err) => Err(format!("keyring delete failed: {err}")),
    }
}

/// Discover a locally-running daemon by reading `<STATE_ROOT>/daemon.json`.
/// Returns `None` when no daemon is running (file absent). Mirrors the
/// version-tolerant parse in `apps/mcp/src/client.ts`.
#[tauri::command]
pub fn client_local_daemon_read() -> Result<Option<LocalDaemon>, String> {
    let Some(root) = daemon_state_root() else {
        return Ok(None);
    };
    let path = root.join("daemon.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("cannot read {}: {err}", path.display())),
    };
    let parsed: DaemonPortFile = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "daemon.json at {} is not valid: {err}. Restart 'posthaste serve'.",
            path.display()
        )
    })?;
    Ok(Some(LocalDaemon {
        port: parsed.port,
        token: parsed.token,
    }))
}

#[cfg(test)]
mod factory_reset_tests {
    use super::*;
    use std::ops::Deref;
    use std::path::Path;
    use tempfile::TempDir;

    struct TempDirGuard(TempDir);

    impl Deref for TempDirGuard {
        type Target = Path;
        fn deref(&self) -> &Path {
            self.0.path()
        }
    }

    impl AsRef<Path> for TempDirGuard {
        fn as_ref(&self) -> &Path {
            self.0.path()
        }
    }

    fn temp_root() -> TempDirGuard {
        TempDirGuard(
            tempfile::Builder::new()
                .prefix("posthaste-desktop-factory-reset-test-")
                .tempdir()
                .expect("temp dir should be created"),
        )
    }

    #[test]
    fn wipe_clears_state_config_and_connections_and_is_idempotent() {
        let base = temp_root();
        let state = base.join("state");
        let config = base.join("config");
        let connections = base.join("connections.json");
        std::fs::create_dir_all(state.join("cache")).unwrap();
        std::fs::write(state.join("mail.sqlite"), b"x").unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("app.toml"), b"x").unwrap();
        std::fs::write(&connections, b"{}").unwrap();

        wipe_factory_reset_targets(&state, Some(&config), Some(&connections)).unwrap();

        assert!(state.exists() && std::fs::read_dir(&state).unwrap().next().is_none());
        assert!(config.exists() && std::fs::read_dir(&config).unwrap().next().is_none());
        assert!(!connections.exists());

        // A second wipe over now-clean targets must not error.
        wipe_factory_reset_targets(&state, Some(&config), Some(&connections)).unwrap();
    }
}
