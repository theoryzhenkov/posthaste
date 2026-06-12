use axum::body::Body;
use axum::http::{header, Request, StatusCode};

use crate::support::{build_app, build_state, status_of, valid_token};

fn bare_request(path: &str) -> axum::http::request::Builder {
    Request::builder().method("GET").uri(path)
}

#[tokio::test]
async fn flag_on_rebinding_host_is_forbidden_even_with_valid_token() {
    // The DNS-rebinding signature: valid token, attacker Host, no Origin.
    // The Origin check alone would wave this through; the Host gate rejects it.
    let app = build_app(build_state(true));
    let request = bare_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .header(header::HOST, "attacker.com")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn flag_on_loopback_host_with_token_succeeds() {
    let app = build_app(build_state(true));
    let request = bare_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .header(header::HOST, "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

#[tokio::test]
async fn flag_on_missing_host_is_forbidden() {
    let app = build_app(build_state(true));
    let request = bare_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn flag_on_bad_host_rejected_before_exempt_route() {
    // Host is validated even for the otherwise-exempt /health route.
    let app = build_app(build_state(true));
    let request = bare_request("/v1/health")
        .header(header::HOST, "attacker.com")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn flag_off_ignores_bad_host() {
    // Safety invariant: with the flag off, even the new Host check never fires.
    let app = build_app(build_state(false));
    let request = bare_request("/v1/settings")
        .header(header::HOST, "attacker.com")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

// -- Exemptions --
