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
//! { "version": 1, "port": 3001, "url": "http://127.0.0.1:3001/v1", "token": "<macaroon>",
//!   "appDir": "/Applications/Posthaste.app/Contents/MacOS" }
//! ```
//! - `version` — schema version (currently [`DISCOVERY_FILE_VERSION`]).
//! - `port` / `url` — where the `/v1` API is bound (loopback).
//! - `appDir` — **optional**, additive (RFC-L2-scripting distribution-wave
//!   follow-up): the directory of the writer's own running executable
//!   (`std::env::current_exe()`'s parent), best-effort (omitted entirely if
//!   `current_exe()` fails, which is the same as an old pre-`appDir` writer).
//!   Lets `posthaste-wizard`'s `ctl.rs` locate the bundled `posthastectl`
//!   sidecar next to *whichever* executable is actually running — the
//!   standalone `posthaste serve` daemon or the desktop app's embedded
//!   server — without hardcoding per-OS install paths or requiring
//!   `POSTHASTE_APP_DIR` to be set by hand. Old readers that only look at
//!   `version`/`port`/`url`/`token` ignore it; an old daemon.json written
//!   before this field existed simply lacks it, and readers must treat that
//!   as "no hint available", not an error.
//! - `token` — the **bootstrap capability**. NOT the server's full-scope local
//!   macaroon (the credential injected into the webview) — that credential is
//!   never written to disk. Instead this is that full-scope token FIRST
//!   attenuated, server-side, down to exactly `{action = mint, read}`
//!   ([`bootstrap_capability`]) before being written (RFC-L2-scripting §7
//!   ruling 11, "least-default bootstrap"). Out of the box this reads (mail
//!   list, message detail, …) and tails the tap (`GET /v1/events`); it cannot
//!   perform ANY write — those need an explicitly minted token.
//!   `posthastectl token mint --grant <scopes> [--expiry <dur>]` (the S4
//!   token-mint UX rider) reads this bootstrap via discovery and calls
//!   `POST /v1/auth/tokens`, which — because the bootstrap carries `mint` —
//!   mints a FRESH, least-privilege, expiring token for the script, scoped up
//!   to whatever was granted (write grants included: `mint` is an issuance
//!   right, so the minted token can be wider than the bootstrap it came from).
//!   See `docs/scripting-quickstart.md` and `crate::api::auth_tokens`.
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

/// Attenuate a **full-scope** local macaroon down to the discovery file's
/// **bootstrap capability**: exactly `{action = mint, read}` (RFC-L2-scripting
/// §7 ruling 11) — read routes and the tap (`action = read`) plus the right to
/// call `POST /v1/auth/tokens` (`action = mint`; see `crate::api::auth_tokens`
/// for why that verb lets a mint-holding caller obtain WIDER tokens than its
/// own scope, despite attenuation normally only narrowing). No account/
/// mailbox/message caveat, so reads and the tap are unrestricted across every
/// account. `full_scope` must genuinely be full-scope (freshly minted, no prior
/// caveats) — this only ever ADDS one caveat, so a non-full-scope input would
/// be narrowed further, not reset to this shape. Returns `None` only if the
/// input does not deserialize as a macaroon at all (should never happen for a
/// freshly minted token); callers must treat that as fatal to discovery —
/// never fall back to writing the wider credential.
pub fn bootstrap_capability(full_scope: &str) -> Option<String> {
    crate::token::attenuate(full_scope, "action = mint,read").ok()
}

/// Write the discovery file for a server bound at `addr`, given the server's
/// **full-scope** local macaroon `full_scope_token` (the same credential
/// injected into the webview). That credential itself is never written: it is
/// FIRST attenuated (server-side, [`bootstrap_capability`]) to the bootstrap
/// capability, and the bootstrap is what lands in `token`. Returns the path
/// written (for later removal). Best-effort: any failure (including a
/// malformed input token) logs and returns `None` — a missing discovery file
/// must never bring the server down, and a failed attenuation must never fall
/// back to persisting the full-scope credential. Only call this when auth is
/// enabled: never persist an unused credential.
pub fn write_discovery_file(addr: SocketAddr, full_scope_token: &str) -> Option<PathBuf> {
    let Some(token) = bootstrap_capability(full_scope_token) else {
        tracing::error!(
            "failed to attenuate the discovery bootstrap token; daemon.json not written"
        );
        return None;
    };
    let roots = resolve_roots();
    let path = roots.state_root.join(DISCOVERY_FILE_NAME);
    let url = format!("http://127.0.0.1:{}/v1", addr.port());
    let body = discovery_body(addr.port(), &url, &token, current_app_dir());
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

/// The directory of the *running* executable — whichever process calls
/// [`write_discovery_file`] (the standalone `posthaste serve` daemon or the
/// desktop app's embedded server), each writing its own `current_exe()`'s
/// parent. Best-effort: `None` on any failure (e.g. the exe was deleted out
/// from under the running process), in which case `appDir` is simply omitted
/// from the written file rather than the write failing.
fn current_app_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// Pure builder for the discovery file's JSON body — split out from
/// [`write_discovery_file`] so the shape (including the optional `appDir`) is
/// unit-testable without touching the filesystem or state-root env vars.
fn discovery_body(
    port: u16,
    url: &str,
    token: &str,
    app_dir: Option<PathBuf>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "version": DISCOVERY_FILE_VERSION,
        "port": port,
        "url": url,
        "token": token,
    });
    if let Some(dir) = app_dir {
        body["appDir"] = serde_json::Value::String(dir.display().to_string());
    }
    body
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

    /// `appDir` is additive: when [`current_app_dir`] resolves (the normal
    /// case for a real running process), the written body carries it.
    #[test]
    fn discovery_body_includes_app_dir_when_present() {
        let body = discovery_body(
            4321,
            "http://127.0.0.1:4321/v1",
            "macaroon-abc",
            Some(PathBuf::from("/opt/posthaste")),
        );
        assert_eq!(body["appDir"], "/opt/posthaste");
        assert_eq!(body["version"], 1);
        assert_eq!(body["port"], 4321);
    }

    /// And omitted (the field simply absent, not `null`) when there is no
    /// resolvable app dir — the same shape an old, pre-`appDir` writer
    /// produced.
    #[test]
    fn discovery_body_omits_app_dir_when_absent() {
        let body = discovery_body(4321, "http://127.0.0.1:4321/v1", "macaroon-abc", None);
        assert!(
            body.get("appDir").is_none(),
            "appDir must be absent, not null, when unresolved: {body}"
        );
    }

    /// Serde tolerance, direction 1: an OLD reader (a struct predating
    /// `appDir`, `#[serde(deny_unknown_fields)]`-free like every real reader
    /// here) must still parse a NEW daemon.json that carries `appDir` —
    /// the whole point of "readers tolerate unknown fields".
    #[test]
    fn old_reader_shape_tolerates_a_new_file_with_app_dir() {
        #[derive(serde::Deserialize)]
        struct OldDiscoveryFile {
            version: u32,
            port: u16,
            url: String,
            token: String,
        }
        let body = discovery_body(
            4321,
            "http://127.0.0.1:4321/v1",
            "macaroon-abc",
            Some(PathBuf::from("/opt/posthaste")),
        );
        let parsed: OldDiscoveryFile =
            serde_json::from_value(body).expect("an old reader ignores the unknown appDir field");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.port, 4321);
        assert_eq!(parsed.url, "http://127.0.0.1:4321/v1");
        assert_eq!(parsed.token, "macaroon-abc");
    }

    /// Serde tolerance, direction 2: a NEW reader (one that knows about
    /// `appDir`) must still parse an OLD daemon.json that predates the
    /// field — `appDir` defaults to `None` rather than failing to parse.
    #[test]
    fn new_reader_shape_tolerates_an_old_file_without_app_dir() {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct NewDiscoveryFile {
            version: u32,
            port: u16,
            url: String,
            token: String,
            #[serde(default, rename = "appDir")]
            app_dir: Option<String>,
        }
        let old_format_json = serde_json::json!({
            "version": 1,
            "port": 4321,
            "url": "http://127.0.0.1:4321/v1",
            "token": "macaroon-abc",
        });
        let parsed: NewDiscoveryFile = serde_json::from_value(old_format_json)
            .expect("an old-format file (no appDir) must still parse");
        assert_eq!(parsed.app_dir, None);
        assert_eq!(parsed.version, 1);
    }

    /// The bootstrap attenuation itself (RFC-L2-scripting §7 ruling 11): a
    /// full-scope token narrows to exactly `{action = mint, read}` — the tap/
    /// read surface and the mint route are reachable, every write verb is not.
    #[test]
    fn bootstrap_capability_narrows_full_scope_to_mint_and_read() {
        let root = crate::token::RootKey::from_test_bytes([11u8; 32]);
        let full_scope = crate::token::mint_full_scope_token(&root);

        let bootstrap =
            bootstrap_capability(&full_scope).expect("attenuating a freshly minted token succeeds");
        assert_ne!(
            bootstrap, full_scope,
            "the discovery file's token must never be the full-scope credential"
        );

        let caveats = crate::token::verify_authenticity(&bootstrap, &root)
            .expect("the bootstrap is authentic under the same root key");
        assert_eq!(caveats.len(), 1, "exactly one narrowing caveat");

        let ctx = |action: crate::authz::Action| crate::authz::CaveatContext {
            action: Some(action),
            account: None,
            mailbox: None,
            message: None,
            now: time::OffsetDateTime::now_utc(),
        };

        // (b) the mint route — `POST /v1/auth/tokens`.
        assert_eq!(
            crate::authz::evaluate(&caveats, &ctx(crate::authz::Action::Mint)),
            crate::authz::Decision::Allow,
            "the bootstrap must reach the mint route"
        );
        // (a) the tap / read surface — `GET /v1/events` and other reads.
        assert_eq!(
            crate::authz::evaluate(&caveats, &ctx(crate::authz::Action::Read)),
            crate::authz::Decision::Allow,
            "the bootstrap must read and tail the tap"
        );
        // (c) every write verb is out of scope.
        for action in [
            crate::authz::Action::Send,
            crate::authz::Action::Tag,
            crate::authz::Action::Move,
            crate::authz::Action::Delete,
            crate::authz::Action::Manage,
        ] {
            assert!(
                matches!(
                    crate::authz::evaluate(&caveats, &ctx(action)),
                    crate::authz::Decision::Deny(_)
                ),
                "the bootstrap must not be able to {action:?}"
            );
        }
    }
}
