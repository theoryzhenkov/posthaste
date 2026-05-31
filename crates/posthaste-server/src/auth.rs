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

use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;
use url::Url;

use crate::api::{ApiError, ApiErrorCode};
use crate::authz::{self, CaveatContext, Decision, ResourceShape, RouteAuthz, ScopeMode};
use crate::token::{self, TokenError};
use crate::AppState;

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

/// Extract and percent-decode the `access_token` query parameter (used by
/// EventSource, which cannot set request headers — see the SSE caveat in the
/// design doc).
///
/// The token is a macaroon (base64; may contain `=` padding and `+`/`/`), and
/// the client builds the URL with `URLSearchParams`, so the value arrives
/// **percent-encoded**. Decode with the matching `application/x-www-form-urlencoded`
/// semantics before comparing — a verbatim scan would mismatch on `%3D` etc.
/// (Regression: SSE auth broke once the token stopped being a bare UUID.)
fn query_token(req: &Request) -> Option<String> {
    let query = req.uri().query()?;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "access_token")
        .map(|(_, value)| value.into_owned())
}

/// Routes (relative to the `/v1` nest, i.e. the path the api router sees) that
/// are exempt from token auth: liveness + browsable docs. Swagger UI is mounted
/// at the app root under `/v1/docs` and never reaches this middleware.
fn is_exempt_path(path: &str) -> bool {
    matches!(
        nest_relative(path),
        "/health" | "/openapi.json" | "/asyncapi.json"
    )
}

/// Whether a route accepts the bearer token via the `access_token` query param.
///
/// This is the single allow-list for token-in-URL: only the **browser-loadable
/// read** routes that cannot set the `Authorization` header — the SSE stream
/// (`EventSource`), account logos and message attachments (`<img>`/downloads).
/// All other routes require the header. NOTE: this only governs where the token
/// may be *presented*; the token still goes through full authenticity +
/// caveat/authz enforcement, so a false match cannot widen access — it would
/// just let a (still-verified, still-scoped) token arrive via the query param.
fn accepts_query_token(path: &str) -> bool {
    let p = nest_relative(path);
    p == "/events"
        || p.starts_with("/account-assets/logos/")
        || (p.starts_with("/sources/") && p.contains("/attachments/"))
}

/// Strip the `/v1` API nest prefix. The auth layer runs on the nested router but
/// `req.uri().path()` is the full path (`/v1/events`), so these route checks
/// must key on the nest-relative path the router declares — the same way the
/// authz-map lookup strips `/v1` from the matched template.
fn nest_relative(path: &str) -> &str {
    path.strip_prefix("/v1").unwrap_or(path)
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
/// - require an authentic macaroon (HMAC chain verified against the root key),
///   accepting the `access_token` query param for `/events` only;
/// - enforce the macaroon's first-party caveats against the matched route's
///   authz descriptor (Stage B): a forged/garbled token is 401, an authentic
///   token whose caveats are out of scope is 403, and a scoped token on an
///   unmapped route fails closed (500).
///
/// @spec docs/eph/DESIGN-L1-trust-model
/// @spec docs/eph/DESIGN-L1-capability-tokens
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

    // Bearer token: from the Authorization header, or the percent-decoded
    // access_token query param for the browser-loadable read routes that cannot
    // set headers (EventSource, <img>) — see `accepts_query_token`.
    let Some(presented) = bearer_token(&req).map(str::to_owned).or_else(|| {
        accepts_query_token(&path)
            .then(|| query_token(&req))
            .flatten()
    }) else {
        return unauthorized().into_response();
    };

    // 1. Authenticity (signature) check. A forged/garbled token is 401; an
    //    authentic-but-attenuated token returns its caveats for enforcement.
    let caveats = match token::verify_authenticity(&presented, &state.macaroon_root_key) {
        Ok(caveats) => caveats,
        Err(TokenError::Malformed) | Err(TokenError::BadSignature) => {
            return unauthorized().into_response();
        }
    };

    // Fast path: a full-scope macaroon (no caveats) is authorized everywhere,
    // exactly as before — no authz-map lookup needed.
    if caveats.is_empty() {
        return next.run(req).await;
    }

    // 2. Resolve the matched route's authz descriptor. `MatchedPath` carries the
    //    FULL template including the `/v1` nest prefix (axum appends the nested
    //    router's template to the outer mount), so strip it to the nest-stripped
    //    template the authz map is keyed on (e.g. `/sources/{source_id}/messages`).
    let Some(matched) = req.extensions().get::<MatchedPath>().map(|m| {
        m.as_str()
            .strip_prefix("/v1")
            .unwrap_or(m.as_str())
            .to_owned()
    }) else {
        // No matched path means no route matched (404 fallback). An attenuated
        // token on a non-route is denied; let routing return its own status by
        // failing closed here.
        return forbidden_scope().into_response();
    };
    let method = req.method().as_str().to_owned();
    let Some(route_authz) = authz::lookup(&method, &matched) else {
        // Unmapped route + scoped token → fail CLOSED as misconfiguration. A new
        // route without an authz entry must never ship open. (Full-scope tokens
        // took the fast path above, so this only blocks attenuated tokens.)
        return misconfigured().into_response();
    };

    // 3. Build the caveat context (path params; query filter for Filter routes).
    let ctx = build_context(&req, &matched, &route_authz);

    // 4. Evaluate every caveat. Any unsatisfied caveat → 403 (authentic but out
    //    of scope), distinct from the 401 above.
    match authz::evaluate(&caveats, &ctx) {
        Decision::Allow => next.run(req).await,
        Decision::Deny(_) => forbidden_scope().into_response(),
    }
}

/// Build the [`CaveatContext`] for a matched, authorized-pending request. For
/// `Gate` routes the resource axes come from path params (matched against the
/// route template); for `Filter` routes they come from the query string. An
/// axis the route does not populate stays `None`, so a caveat restricting it is
/// unsatisfiable and the request is denied — the fail-closed rule.
fn build_context(req: &Request, template: &str, authz: &RouteAuthz) -> CaveatContext {
    // `template` is nest-stripped (no `/v1`); strip the same prefix from the
    // concrete path so segment matching lines up.
    let raw_path = req.uri().path();
    let path = raw_path.strip_prefix("/v1").unwrap_or(raw_path);
    let (account, mailbox, message) = match authz.mode {
        ScopeMode::Gate => extract_path_axes(template, path, &authz.resource),
        ScopeMode::Filter => extract_query_axes(req.uri().query(), &authz.resource),
    };
    CaveatContext {
        action: authz.action,
        account,
        mailbox,
        message,
        now: OffsetDateTime::now_utc(),
    }
}

/// Resolve the `(account, mailbox, message)` axes from path params by matching
/// the request path against the route template segment-by-segment.
fn extract_path_axes(
    template: &str,
    path: &str,
    shape: &ResourceShape,
) -> (Option<String>, Option<String>, Option<String>) {
    let params = path_params(template, path);
    let pick = |name: Option<&str>| name.and_then(|n| params.get(n).cloned());
    (
        pick(shape.account),
        pick(shape.mailbox),
        pick(shape.message),
    )
}

/// Match a route template (`/sources/{source_id}/messages/{message_id}`) against
/// a concrete path, returning the captured `{param}` values. Returns an empty
/// map on any segment-count mismatch (the templates always match here, since the
/// router selected this template).
fn path_params(template: &str, path: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    let template_segments: Vec<&str> = template.trim_matches('/').split('/').collect();
    let path_segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    if template_segments.len() != path_segments.len() {
        return params;
    }
    for (tpl, actual) in template_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = tpl.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            // Path segments are percent-encoded on the wire; decode so caveat
            // comparison is against the logical id.
            let decoded = percent_decode(actual);
            params.insert(name.to_string(), decoded);
        }
    }
    params
}

/// Resolve the `(account, mailbox, message)` axes from query params for a
/// `Filter` route. A missing filter leaves the axis `None`, so a resource caveat
/// restricting it is unsatisfiable → the request is denied (the matching-filter
/// rule). `message` is never a query filter in this API.
fn extract_query_axes(
    query: Option<&str>,
    shape: &ResourceShape,
) -> (Option<String>, Option<String>, Option<String>) {
    let pick = |name: Option<&str>| name.and_then(|n| query_param(query, n));
    (
        pick(shape.account),
        pick(shape.mailbox),
        pick(shape.message),
    )
}

/// Read a single query parameter's (percent-decoded) value. Fails CLOSED on a
/// DUPLICATE key: if the query string carries `name` more than once, returns
/// `None` (so the resource caveat is unsatisfiable → deny) rather than taking
/// the first occurrence. This prevents any middleware-vs-handler disagreement on
/// `?sourceId=a&sourceId=b` (the middleware must never authorize a value the
/// handler might not use).
fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let query = query?;
    let mut found: Option<String> = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == name {
            if found.is_some() {
                // Duplicate key — ambiguous; fail closed.
                return None;
            }
            found = Some(percent_decode(value));
        }
    }
    found
}

/// Minimal percent-decoder for path/query segment values. Decodes `%XX` escapes
/// and `+` (in query position both `+` and `%20` mean space). On any malformed
/// escape the original byte is preserved.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

/// 403: the token is authentic but a caveat is not satisfied (out of scope).
fn forbidden_scope() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        ApiErrorCode::Forbidden,
        "token is not authorized for this request",
    )
}

/// 500: a scoped token reached a route with no authz-map entry. Failing closed
/// here means a newly added, unmapped route denies attenuated tokens rather than
/// silently granting them.
fn misconfigured() -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiErrorCode::InternalError,
        "route is not present in the authorization map",
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
    fn route_checks_handle_the_v1_nest_prefix() {
        // The auth layer runs on the nested router but sees the full request
        // path, so the SSE stream arrives as `/v1/events`. Regression: it was
        // only matched as `/events`, so EventSource's `access_token` query token
        // was dropped and `/events` 401'd under require_auth, killing live
        // updates (archived/deleted rows lingered until a manual refresh).
        assert!(accepts_query_token("/v1/events"));
        assert!(accepts_query_token("/events"));
        // Browser-loadable read routes (<img>) also accept the query-param token.
        assert!(accepts_query_token("/v1/account-assets/logos/img-1"));
        assert!(accepts_query_token(
            "/v1/sources/acct/messages/m1/attachments/a1"
        ));
        // Everything else requires the Authorization header.
        assert!(!accepts_query_token("/v1/messages"));
        assert!(!accepts_query_token("/v1/settings"));
        assert!(!accepts_query_token("/v1/accounts"));

        assert!(is_exempt_path("/v1/openapi.json"));
        assert!(is_exempt_path("/v1/health"));
        assert!(is_exempt_path("/health"));
        assert!(!is_exempt_path("/v1/sources"));
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
