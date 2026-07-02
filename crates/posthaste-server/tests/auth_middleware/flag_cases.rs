use axum::body::Body;
use axum::http::{header, StatusCode};
use posthaste_api::token::{mint_full_scope_token, RootKey};

use crate::support::{build_app, build_state, get_request, status_of, valid_token, CORS_ORIGIN};

#[tokio::test]
async fn flag_off_request_without_token_succeeds() {
    let app = build_app(build_state(false));
    let status = status_of(
        app,
        get_request("/v1/settings").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "with require_auth off, an un-tokened request must still succeed"
    );
}

#[tokio::test]
async fn flag_off_ignores_bad_origin() {
    let app = build_app(build_state(false));
    let request = get_request("/v1/settings")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

// -- Flag on: token enforcement --

#[tokio::test]
async fn flag_on_no_token_is_unauthorized() {
    let app = build_app(build_state(true));
    let status = status_of(
        app,
        get_request("/v1/settings").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn flag_on_correct_token_succeeds() {
    let app = build_app(build_state(true));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

#[tokio::test]
async fn flag_on_wrong_token_is_unauthorized() {
    // A garbage (non-macaroon) token must not verify.
    let app = build_app(build_state(true));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, "Bearer not-the-token")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn flag_on_macaroon_from_other_root_key_is_unauthorized() {
    // A well-formed macaroon minted under a DIFFERENT root key fails the HMAC
    // verification, even though it deserializes fine.
    let app = build_app(build_state(true));
    let foreign = mint_full_scope_token(&RootKey::from_test_bytes([1u8; 32]));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {foreign}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn flag_on_bad_origin_is_forbidden() {
    let app = build_app(build_state(true));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn flag_on_allowed_origin_with_token_succeeds() {
    let app = build_app(build_state(true));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .header(header::ORIGIN, CORS_ORIGIN)
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

#[tokio::test]
async fn flag_on_tauri_origin_with_token_succeeds() {
    let app = build_app(build_state(true));
    let request = get_request("/v1/settings")
        .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
        .header(header::ORIGIN, "tauri://localhost")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

// -- Host / DNS-rebinding defense (H1) --
