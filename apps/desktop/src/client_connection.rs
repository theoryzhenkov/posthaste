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
use tauri::{AppHandle, Manager, Runtime};

/// Keyring service name for client-held per-profile remote tokens. Kept
/// distinct from the daemon's `posthaste` service so client and daemon secrets
/// never collide.
const CLIENT_KEYRING_SERVICE: &str = "posthaste-client";

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

/// Persist the connection-profile store. The frontend serializes the
/// `ConnectionsFile` (which holds NO secrets — tokens live in the keyring).
#[tauri::command]
pub fn client_connections_write<R: Runtime>(
    app: AppHandle<R>,
    contents: String,
) -> Result<(), String> {
    let dir = client_dir(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let path = dir.join(CONNECTIONS_FILE);
    std::fs::write(&path, contents).map_err(|err| format!("cannot write {}: {err}", path.display()))
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
