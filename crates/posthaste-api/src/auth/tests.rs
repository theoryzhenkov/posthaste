use super::*;

#[test]
fn constant_time_eq_matches_byte_equality() {
    assert!(constant_time_eq(b"token", b"token"));
    assert!(!constant_time_eq(b"token", b"toker"));
    assert!(!constant_time_eq(b"token", b"tok"));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn exempt_path_check_handles_the_v1_nest_prefix() {
    // The auth layer runs on the nested router but sees the full request
    // path, so liveness/doc routes arrive as `/v1/health` etc.; the check
    // must strip the nest prefix (it also tolerates the bare form).
    assert!(is_exempt_path("/v1/openapi.json"));
    assert!(is_exempt_path("/v1/asyncapi.json"));
    assert!(is_exempt_path("/v1/health"));
    assert!(is_exempt_path("/health"));
    // The OAuth loopback callback is the provider's browser redirect: it
    // cannot carry a bearer token and is authenticated by the `state` param.
    assert!(is_exempt_path("/v1/oauth/callback"));
    assert!(is_exempt_path("/oauth/callback"));
    // The OAuth *start* endpoints are app-initiated (token-bearing) and must
    // stay behind auth.
    assert!(!is_exempt_path("/v1/oauth/start"));
    // Everything else requires an authentic token in the Authorization
    // header — including the previously query-token routes (events, logos,
    // attachments), which now authenticate via header (fetch/blob fetch).
    assert!(!is_exempt_path("/v1/account-assets/logos/img-1"));
    assert!(!is_exempt_path("/v1/events"));
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
