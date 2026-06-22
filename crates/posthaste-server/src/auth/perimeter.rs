use axum::extract::Request;
use axum::http::header;
use url::Url;

/// Loopback hosts always allowed in the `Host` header, regardless of the
/// configured bind address. These are the legitimate names for a daemon bound
/// to loopback; an attacker-controlled rebinding domain will not be in this set.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Constant-time byte equality. Compares the full length of both inputs
/// without early-exit on the first mismatch, so timing does not leak how many
/// leading bytes matched. Differing lengths short-circuit to `false` (length
/// is not itself secret), but equal-length inputs are compared in full.
///
/// No longer the token gate — that is now a macaroon HMAC verification (see
/// `require_auth_layer`). Retained for Stage B caveat enforcement (e.g.
/// comparing caveat-extracted identifiers) and covered by a unit test.
#[allow(dead_code)]
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
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
pub(crate) fn bearer_token(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(rest.trim())
}

/// Routes (relative to the `/v1` nest, i.e. the path the api router sees) that
/// are exempt from token auth: liveness + browsable docs, plus the OAuth
/// loopback callback. Swagger UI is mounted at the app root under `/v1/docs` and
/// never reaches this middleware.
///
/// `/oauth/callback` is the provider's browser redirect target: a top-level
/// navigation that carries neither the `Authorization` header nor an
/// app-controlled `Origin`, so it cannot present a bearer token. It is
/// authenticated instead by the unguessable `state` parameter, which the server
/// correlates to a backend-held PKCE flow (see `complete_account_oauth`); an
/// unknown or already-used `state` is rejected there. The mandatory `Host`
/// allowlist still runs before this exemption.
pub(crate) fn is_exempt_path(path: &str) -> bool {
    matches!(
        nest_relative(path),
        "/health" | "/openapi.json" | "/asyncapi.json" | "/oauth/callback"
    )
}

/// Strip the `/v1` API nest prefix. The auth layer runs on the nested router but
/// `req.uri().path()` is the full path (`/v1/events`), so these route checks
/// must key on the nest-relative path the router declares — the same way the
/// authz-map lookup strips `/v1` from the matched template.
pub(crate) fn nest_relative(path: &str) -> &str {
    path.strip_prefix("/v1").unwrap_or(path)
}

/// Validate that a browser-supplied `Origin`/`Referer` matches the allowlist.
///
/// Both sides are reduced to their canonical `scheme://host[:port]` origin via
/// `url::Url`, so a `Referer` path/query cannot defeat the check and casing /
/// trailing-dot quirks are normalized by the parser. Fail-closed: a value that
/// does not parse as an absolute URL is rejected.
pub(crate) fn origin_allowed(value: &str, allowed: &[String]) -> bool {
    let Some(origin) = canonical_origin(value) else {
        return false;
    };
    allowed.iter().any(|candidate| candidate == &origin)
}

/// Reduce a URL (Origin or Referer) to its canonical `scheme://host[:port]`
/// ASCII origin string using `url::Url`. Returns `None` when the value is not
/// an absolute URL with a host, so callers fail closed.
pub(crate) fn canonical_origin(value: &str) -> Option<String> {
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
pub(crate) fn bind_host(bind_address: &str) -> Option<&str> {
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
pub(crate) fn normalize_host_header(value: &str) -> Option<String> {
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
pub(crate) fn host_allowed(req: &Request, allowed: &[String]) -> bool {
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
