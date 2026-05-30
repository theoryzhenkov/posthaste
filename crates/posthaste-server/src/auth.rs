//! Loopback bearer-token + Origin/Host guard for the `/v1` API.
//!
//! Gated behind `[daemon] require_auth` (default `true`). When the flag is
//! off (explicit opt-out) the middleware is a pass-through, so the no-auth
//! behavior is byte-identical. When on:
//!
//! - The `Host` header is validated against an allowlist on **every** request
//!   (including the otherwise-exempt liveness/doc routes), independent of
//!   `Origin`. This is the load-bearing DNS-rebinding defense: a rebinding
//!   attack reaches us as `Host: attacker.com` with no `Origin`, which the
//!   Origin check alone would wave through.
//! - A matching `Authorization: Bearer <token>` (the per-process token) is
//!   required, except for a small set of exempt liveness/doc routes.
//! - Browser requests carrying an `Origin`/`Referer` are additionally checked
//!   against an origin allowlist (CSRF defense-in-depth).
//!
//! @spec docs/eph/DESIGN-L1-trust-model

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use url::Url;

use crate::api::{ApiError, ApiErrorCode};
use crate::AppState;

/// Loopback hosts always allowed in the `Host` header, regardless of the
/// configured bind address. These are the legitimate names for a daemon bound
/// to loopback; an attacker-controlled rebinding domain will not be in this set.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Constant-time byte equality. Compares the full length of both inputs
/// without early-exit on the first mismatch, so timing does not leak how many
/// leading bytes matched. Differing lengths short-circuit to `false` (length
/// is not itself secret), but equal-length inputs are compared in full.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the bearer token from an `Authorization: Bearer <token>` header.
fn bearer_token(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(rest.trim())
}

/// Extract the `access_token` query parameter (used by EventSource, which
/// cannot set request headers — see the SSE caveat in the design doc).
///
/// The token is a UUID/opaque ASCII string with no reserved characters, so a
/// simple `key=value` scan over the query string suffices; we only need to
/// recover the verbatim token to compare it.
fn query_token(req: &Request) -> Option<String> {
    let query = req.uri().query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "access_token").then(|| value.to_string())
    })
}

/// Routes (relative to the `/v1` nest, i.e. the path the api router sees) that
/// are exempt from token auth: liveness + browsable docs. Swagger UI is mounted
/// at the app root under `/v1/docs` and never reaches this middleware.
fn is_exempt_path(path: &str) -> bool {
    matches!(path, "/health" | "/openapi.json" | "/asyncapi.json")
}

/// Whether the request targets the SSE event stream, which accepts a token via
/// the `access_token` query param because `EventSource` cannot set headers.
fn is_events_path(path: &str) -> bool {
    path == "/events"
}

/// Validate that a browser-supplied `Origin`/`Referer` matches the allowlist.
///
/// Both sides are reduced to their canonical `scheme://host[:port]` origin via
/// `url::Url`, so a `Referer` path/query cannot defeat the check and casing /
/// trailing-dot quirks are normalized by the parser. Fail-closed: a value that
/// does not parse as an absolute URL is rejected.
fn origin_allowed(value: &str, allowed: &[String]) -> bool {
    let Some(origin) = canonical_origin(value) else {
        return false;
    };
    allowed.iter().any(|candidate| candidate == &origin)
}

/// Reduce a URL (Origin or Referer) to its canonical `scheme://host[:port]`
/// ASCII origin string using `url::Url`. Returns `None` when the value is not
/// an absolute URL with a host, so callers fail closed.
fn canonical_origin(value: &str) -> Option<String> {
    let url = Url::parse(value.trim()).ok()?;
    let scheme = url.scheme();
    let host = url.host_str()?;
    // `url` lowercases the scheme and host and normalizes IDN/trailing dots.
    match url.port() {
        Some(port) => Some(format!("{scheme}://{host}:{port}")),
        None => Some(format!("{scheme}://{host}")),
    }
}

/// Build the Origin allowlist from the **same source as CORS**: the configured
/// CORS origin plus the host's extra CORS origins (the desktop declares its real
/// per-platform Tauri webview origins there — `tauri://localhost`,
/// `https://tauri.localhost` on Windows, etc.). Deriving both from one source
/// keeps the auth check and CORS from drifting (a divergence would 403 a
/// first-party client whose origin CORS allows). Each entry is canonicalized so
/// comparisons are exact-string against canonical forms; unparseable entries are
/// dropped (they could never match anyway).
pub fn origin_allowlist(cors_origin: &str, extra_origins: &[String]) -> Vec<String> {
    let mut allowed: Vec<String> = canonical_origin(cors_origin).into_iter().collect();
    for extra in extra_origins {
        if let Some(origin) = canonical_origin(extra) {
            allowed.push(origin);
        }
    }
    allowed
}

/// Build the `Host`-header allowlist: the loopback names plus the configured
/// bind host (if it is a real host and not a wildcard). Hosts are stored
/// lowercase, without port; the request-time check is host-only.
///
/// `bind_address` is a `host:port` string (e.g. `127.0.0.1:3001`) or an
/// override such as `127.0.0.1:0`. A wildcard bind host (`0.0.0.0` / `::`) is
/// not added — binding to all interfaces does not make any external name a
/// legitimate `Host`, so only the loopback names remain trusted.
pub fn host_allowlist(bind_address: &str) -> Vec<String> {
    let mut allowed: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| h.to_string()).collect();
    if let Some(host) = bind_host(bind_address) {
        let host = host.to_ascii_lowercase();
        let is_wildcard = host == "0.0.0.0" || host == "::" || host == "[::]";
        if !is_wildcard && !allowed.contains(&host) {
            allowed.push(host);
        }
    }
    allowed
}

/// Extract the host portion from a `host:port` bind address, handling
/// bracketed IPv6 (`[::1]:0`) and bare hosts. Returns the host without
/// brackets or port.
fn bind_host(bind_address: &str) -> Option<&str> {
    let trimmed = bind_address.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        // Bracketed IPv6: `[::1]:port` or `[::1]`.
        return rest.split(']').next();
    }
    // Bare host or IPv4: split off a trailing `:port` if present. A bare IPv6
    // without brackets is ambiguous, but bind addresses use the bracket form.
    match trimmed.rsplit_once(':') {
        Some((host, _port)) if !host.contains(':') => Some(host),
        _ => Some(trimmed),
    }
}

/// Reduce a `Host` header value to its host portion: lowercase, trailing dot
/// stripped, port ignored, IPv6 brackets removed. Returns `None` if empty.
fn normalize_host_header(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let without_port = if let Some(rest) = trimmed.strip_prefix('[') {
        // `[::1]:port` / `[::1]` — take what is inside the brackets.
        rest.split(']').next().unwrap_or(rest)
    } else {
        match trimmed.rsplit_once(':') {
            // IPv4/host with a port: strip it. Reject ambiguous bare IPv6.
            Some((host, _port)) if !host.contains(':') => host,
            _ => trimmed,
        }
    };
    let normalized = without_port.trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Whether the request's `Host` header is present and allowlisted. A missing
/// `Host` is rejected (HTTP/1.1 mandates it; absence is the rebinding/raw-socket
/// signature). This is the primary DNS-rebinding defense and runs before any
/// route exemption.
fn host_allowed(req: &Request, allowed: &[String]) -> bool {
    let Some(raw) = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(host) = normalize_host_header(raw) else {
        return false;
    };
    allowed.iter().any(|candidate| candidate == &host)
}

/// Axum middleware enforcing the loopback trust model on the `/v1` api router.
///
/// Pass-through when `state.require_auth` is `false`. Otherwise, in order:
/// - validate the `Host` header against the allowlist on **every** request,
///   before any exemption (the DNS-rebinding defense);
/// - exempt liveness/doc routes from token auth;
/// - reject browser requests whose `Origin`/`Referer` is not allowlisted;
/// - require a matching bearer token (constant-time compare), accepting the
///   `access_token` query param for `/events` only.
///
/// @spec docs/eph/DESIGN-L1-trust-model
pub async fn require_auth_layer(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    if !state.require_auth {
        return next.run(req).await;
    }

    // Host validation runs first, for ALL routes including the exempt ones. A
    // DNS-rebinding attack arrives as `Host: attacker.com` with no `Origin`, so
    // the Origin check alone is insufficient; a mandatory Host allowlist is the
    // real defense. A missing Host is also rejected.
    if !host_allowed(&req, &state.host_allowlist) {
        return forbidden().into_response();
    }

    let path = req.uri().path().to_string();
    if is_exempt_path(&path) {
        return next.run(req).await;
    }

    // Origin/Referer defense-in-depth (CSRF): a token can be read from the page
    // context under rebinding, so a browser-supplied Origin/Referer must also be
    // allowlisted. Non-browser clients send neither and pass on token alone.
    let origin_header = req
        .headers()
        .get(header::ORIGIN)
        .or_else(|| req.headers().get(header::REFERER))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if let Some(origin) = origin_header {
        if !origin_allowed(&origin, &state.origin_allowlist) {
            return forbidden().into_response();
        }
    }

    // Bearer token: from the Authorization header, or the access_token query
    // param for the SSE stream only (EventSource cannot set headers).
    let presented = bearer_token(&req)
        .map(str::to_owned)
        .or_else(|| is_events_path(&path).then(|| query_token(&req)).flatten());

    let authorized = presented
        .as_deref()
        .is_some_and(|token| constant_time_eq(token.as_bytes(), state.auth_token.as_bytes()));

    if !authorized {
        return unauthorized().into_response();
    }

    next.run(req).await
}

fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        ApiErrorCode::Unauthorized,
        "missing or invalid bearer token",
    )
}

fn forbidden() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        ApiErrorCode::Forbidden,
        "request origin is not allowed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_byte_equality() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"toker"));
        assert!(!constant_time_eq(b"token", b"tok"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn origin_allowed_normalizes_and_matches() {
        // The allowlist is built from the CORS origin + the host's extra CORS
        // origins (what the desktop declares per platform).
        let allowed = origin_allowlist(
            "http://localhost:5173",
            &[
                "tauri://localhost".to_string(),
                "https://tauri.localhost".to_string(),
                "http://127.0.0.1:5173".to_string(),
            ],
        );
        assert!(origin_allowed("http://localhost:5173", &allowed));
        assert!(origin_allowed("http://localhost:5173/some/path", &allowed));
        // Casing is normalized by url::Url.
        assert!(origin_allowed("HTTP://LOCALHOST:5173", &allowed));
        assert!(origin_allowed("tauri://localhost", &allowed));
        // The Windows WebView2 origin must be allowed (regression: it was
        // previously missing from the hardcoded list).
        assert!(origin_allowed("https://tauri.localhost", &allowed));
        assert!(!origin_allowed("http://evil.example", &allowed));
        assert!(!origin_allowed("http://localhost:9999", &allowed));
        // An origin CORS does not list (e.g. plain http://tauri.localhost here)
        // is not allowed — auth tracks CORS exactly.
        assert!(!origin_allowed("http://tauri.localhost", &allowed));
        // Fail closed on garbage / non-absolute values.
        assert!(!origin_allowed("not a url", &allowed));
        assert!(!origin_allowed("", &allowed));
    }

    #[test]
    fn host_allowlist_includes_loopback_and_bind_host() {
        let allowed = host_allowlist("127.0.0.1:3001");
        assert!(allowed.contains(&"localhost".to_string()));
        assert!(allowed.contains(&"127.0.0.1".to_string()));
        assert!(allowed.contains(&"::1".to_string()));
    }

    #[test]
    fn host_allowlist_adds_custom_bind_host_but_not_wildcard() {
        let custom = host_allowlist("my-host.internal:8080");
        assert!(custom.contains(&"my-host.internal".to_string()));

        let wildcard = host_allowlist("0.0.0.0:3001");
        assert!(!wildcard.contains(&"0.0.0.0".to_string()));
        // Loopback names remain trusted even with a wildcard bind.
        assert!(wildcard.contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn bind_host_parses_ipv4_ipv6_and_bare() {
        assert_eq!(bind_host("127.0.0.1:3001"), Some("127.0.0.1"));
        assert_eq!(bind_host("[::1]:0"), Some("::1"));
        assert_eq!(bind_host("localhost"), Some("localhost"));
    }

    #[test]
    fn normalize_host_header_strips_port_dot_and_case() {
        assert_eq!(
            normalize_host_header("127.0.0.1:3001").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            normalize_host_header("LocalHost").as_deref(),
            Some("localhost")
        );
        assert_eq!(
            normalize_host_header("localhost.").as_deref(),
            Some("localhost")
        );
        assert_eq!(normalize_host_header("[::1]:8080").as_deref(), Some("::1"));
        assert_eq!(normalize_host_header(""), None);
    }
}
