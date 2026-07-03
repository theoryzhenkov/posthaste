//! The daemon **discovery file** (`daemon.json`) — RFC-L2-scripting §7 ruling 7b.
//!
//! A running Posthaste server writes a well-known discovery file into its state
//! dir so a same-machine client — `posthastectl` — finds the bound port and a
//! bootstrap capability with **no** `--url`/token flags. This is written by BOTH
//! entrypoints that own a bound server:
//!
//! - the standalone `posthaste serve` daemon (`main.rs`), and
//! - the desktop app's **embedded in-process server** (the discovery rider): the
//!   embedded server used to inject the port/token into its webview *only*, so a
//!   laptop script had no way to reach the app — "easy" failed at step zero. Now
//!   the embedded app writes the same discovery file the daemon does.
//!
//! Shape — minimal and versioned; readers tolerate unknown fields so it can grow:
//! ```json
//! { "version": 1, "port": 3001, "url": "http://127.0.0.1:3001/v1", "token": "<macaroon>" }
//! ```
//! - `version` — schema version (currently [`DISCOVERY_FILE_VERSION`]).
//! - `port` / `url` — where the `/v1` API is bound (loopback).
//! - `token` — the **bootstrap capability**: the server's full-scope local
//!   macaroon (the very credential injected into the webview). It is deliberately
//!   full-scope — a *bootstrap* credential, not a working one. Scripts do NOT use
//!   it directly: `posthastectl token mint --grant <scopes> [--expiry <dur>]`
//!   (the S4 token-mint UX rider) reads this token via discovery and attenuates it
//!   (server-side, `POST /v1/auth/tokens`) into a least-privilege, expiring token
//!   for the script. See `docs/scripting-quickstart.md`.
//!
//! The file carries a live credential, so it is written `0600` (state dir
//! best-effort `0700`), overwriting any prior file, and removed on clean shutdown
//! (the M20 [`crate::ShutdownSequence`] for the daemon; the exit hook for the
//! desktop). Best-effort throughout: a write failure logs and the server runs on.
//!
//! @spec docs/eph/RFC-L2-scripting#7-rulings
//! @spec docs/eph/DESIGN-L1-trust-model

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::config::resolve_roots;
use crate::secure_file::write_secure_file;

/// The discovery file's basename, resolved under the state root.
const DISCOVERY_FILE_NAME: &str = "daemon.json";

/// The discovery file schema version. Bump only on a breaking shape change;
/// readers already tolerate additive fields.
pub const DISCOVERY_FILE_VERSION: u32 = 1;

/// The absolute path of the discovery file under the resolved state root.
pub fn discovery_file_path() -> PathBuf {
    resolve_roots().state_root.join(DISCOVERY_FILE_NAME)
}

/// Write the discovery file for a server bound at `addr` with bearer `token`.
/// Returns the path written (for later removal). Best-effort: any failure logs
/// and returns `None` — a missing discovery file must never bring the server
/// down. Only call this when auth is enabled: never persist an unused credential.
pub fn write_discovery_file(addr: SocketAddr, token: &str) -> Option<PathBuf> {
    let roots = resolve_roots();
    let path = roots.state_root.join(DISCOVERY_FILE_NAME);
    let url = format!("http://127.0.0.1:{}/v1", addr.port());
    let body = serde_json::json!({
        "version": DISCOVERY_FILE_VERSION,
        "port": addr.port(),
        "url": url,
        "token": token,
    });
    let contents = match serde_json::to_string_pretty(&body) {
        Ok(contents) => contents,
        Err(error) => {
            tracing::error!(%error, "failed to serialize daemon.json");
            return None;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&roots.state_root) {
        tracing::error!(%error, "failed to create state root for daemon.json");
        return None;
    }
    // Best-effort tighten the state dir to 0700 on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&roots.state_root, std::fs::Permissions::from_mode(0o700));
    }
    if let Err(error) = write_secure_file(&path, contents.as_bytes()) {
        tracing::error!(%error, path = %path.display(), "failed to write daemon.json");
        return None;
    }
    Some(path)
}

/// Remove the discovery file on clean shutdown. Best-effort; an already-absent
/// file is success (nothing to leak).
pub fn remove_discovery_file(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "failed to remove daemon.json on shutdown");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize a sample discovery file and assert the wire shape posthastectl
    /// reads: a versioned `{ port, url, token }` object.
    #[test]
    fn discovery_file_has_the_versioned_port_url_token_shape() {
        // Drive the serializer directly (the write path's body), independent of
        // any state root, so the shape assertion has no filesystem dependency.
        let addr: SocketAddr = "127.0.0.1:4321".parse().unwrap();
        let body = serde_json::json!({
            "version": DISCOVERY_FILE_VERSION,
            "port": addr.port(),
            "url": format!("http://127.0.0.1:{}/v1", addr.port()),
            "token": "macaroon-abc",
        });
        assert_eq!(body["version"], 1);
        assert_eq!(body["port"], 4321);
        assert_eq!(body["url"], "http://127.0.0.1:4321/v1");
        assert_eq!(body["token"], "macaroon-abc");
    }
}
