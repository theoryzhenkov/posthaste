//! Maintenance hooks over the backend's [`AppPaths`]: the next-launch repair
//! and factory-reset markers, the diagnostics info bundle, and the log-folder
//! reveal.

use posthaste_client_backend::{AppPaths, REPAIR_MARKER_FILE};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

/// Marker that requests a full factory reset on the next launch.
const FACTORY_RESET_MARKER: &str = ".factory-reset-requested";

/// Request a local-database repair on the next launch.
///
/// Writes the marker file the embedded backend's store checks on open
/// ([`REPAIR_MARKER_FILE`]). On restart the corrupt database is quarantined
/// and rebuilt from config; accounts and secrets are unaffected. The frontend
/// calls this and then relaunches the app.
#[tauri::command]
pub(crate) fn request_database_repair() -> Result<(), String> {
    let state_root = AppPaths::resolve().state_root;
    std::fs::create_dir_all(&state_root)
        .map_err(|err| format!("cannot create {}: {err}", state_root.display()))?;
    let marker = state_root.join(REPAIR_MARKER_FILE);
    std::fs::write(&marker, b"")
        .map_err(|err| format!("cannot write {}: {err}", marker.display()))?;
    Ok(())
}

/// Request a full factory reset on the next launch.
///
/// Writes the marker [`consume_factory_reset_marker`] acts on at startup
/// (before the embedded backend opens anything). The frontend also clears its
/// local UI state, then relaunches. Unlike repair this also removes accounts
/// + config; the user starts from a clean install.
#[tauri::command]
pub(crate) fn request_factory_reset() -> Result<(), String> {
    let state_root = AppPaths::resolve().state_root;
    std::fs::create_dir_all(&state_root)
        .map_err(|err| format!("cannot create {}: {err}", state_root.display()))?;
    let marker = state_root.join(FACTORY_RESET_MARKER);
    std::fs::write(&marker, b"")
        .map_err(|err| format!("cannot write {}: {err}", marker.display()))?;
    Ok(())
}

/// If a factory reset was requested, perform it before the embedded backend
/// starts (so nothing holds the files open) and return `true`. Best-effort: a
/// partial wipe must not block launch. Call once at startup, before the
/// backend assembles.
pub(crate) fn consume_factory_reset_marker(paths: &AppPaths) -> bool {
    if !paths.state_root.join(FACTORY_RESET_MARKER).exists() {
        return false;
    }
    let _ = wipe_factory_reset_targets(paths);
    true
}

/// Wipe everything a factory reset owns: the state root (mail.sqlite, body
/// files, connection info, logs) and the config root (app.toml, accounts,
/// smart mailboxes). Each root is removed then recreated empty; missing paths
/// are a no-op (idempotent). Keyring secrets are left as harmless orphans
/// (re-adding an account overwrites them).
fn wipe_factory_reset_targets(paths: &AppPaths) -> std::io::Result<()> {
    for root in [&paths.state_root, &paths.config_root] {
        if root.exists() {
            std::fs::remove_dir_all(root)?;
        }
        std::fs::create_dir_all(root)?;
    }
    Ok(())
}

/// Sanitized runtime info for the "Copy diagnostics" support bundle: the app
/// version, OS/arch, and the log directory path. No secrets, message bodies,
/// or account data cross this boundary — account status is gathered
/// renderer-side from the accounts query.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticsInfo {
    pub(crate) app_version: String,
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) log_dir_path: Option<String>,
}

/// Return sanitized runtime info for the diagnostics bundle.
#[tauri::command]
pub(crate) fn get_diagnostics_info<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DiagnosticsInfo, String> {
    let log_dir_path = Some(log_dir().to_string_lossy().into_owned());
    Ok(DiagnosticsInfo {
        app_version: app.package_info().version.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        log_dir_path,
    })
}

/// Open the log directory in the OS file manager so the user can attach the
/// JSONL logs to a bug report. Best-effort: a missing directory is created;
/// an open failure returns an error string the renderer toasts.
#[tauri::command]
pub(crate) fn reveal_log_folder() -> Result<(), String> {
    let log_dir = log_dir();
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

pub(crate) fn log_dir() -> std::path::PathBuf {
    AppPaths::resolve().state_root.join("logs")
}
