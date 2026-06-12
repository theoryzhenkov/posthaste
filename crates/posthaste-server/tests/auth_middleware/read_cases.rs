use axum::body::Body;
use axum::http::{header, StatusCode};

use crate::support::{build_app, build_state, get_request, status_of, valid_token};

#[tokio::test]
async fn flag_on_health_succeeds_without_token() {
    let app = build_app(build_state(true));
    let status = status_of(app, get_request("/v1/health").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn flag_on_openapi_doc_succeeds_without_token() {
    let app = build_app(build_state(true));
    let status = status_of(
        app,
        get_request("/v1/openapi.json").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// -- Browser-loadable reads authenticate via the Authorization header --
//
// The SSE stream (fetchEventSource), account logos and message attachments are
// all loaded with a `fetch()` that sets the `Authorization` header, so they go
// through the same header gate as every other route. The token is never carried
// in a URL: the `?access_token=` query param is honored nowhere.

#[tokio::test]
async fn flag_on_browser_loadable_reads_succeed_with_header_token() {
    for path in [
        "/v1/events",
        "/v1/account-assets/logos/img-1",
        "/v1/sources/acct/messages/m1/attachments/a1",
    ] {
        let request = get_request(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", valid_token()))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status_of(build_app(build_state(true)), request).await,
            StatusCode::OK,
            "{path} should authenticate via the Authorization header"
        );
    }
}

#[tokio::test]
async fn flag_on_browser_loadable_reads_reject_missing_token() {
    for path in [
        "/v1/events",
        "/v1/account-assets/logos/img-1",
        "/v1/sources/acct/messages/m1/attachments/a1",
    ] {
        let request = get_request(path).body(Body::empty()).unwrap();
        assert_eq!(
            status_of(build_app(build_state(true)), request).await,
            StatusCode::UNAUTHORIZED,
            "{path} without a token must 401"
        );
    }
}

#[tokio::test]
async fn flag_on_access_token_query_param_is_not_honored() {
    // The query-param token transport was removed entirely: a valid token in
    // `?access_token=` with no Authorization header must NOT authenticate, on
    // any route (including the formerly-exempt /events + logo/attachment paths).
    for path in [
        "/v1/events",
        "/v1/account-assets/logos/img-1",
        "/v1/sources/acct/messages/m1/attachments/a1",
        "/v1/settings",
    ] {
        let request = get_request(&format!("{path}?access_token={}", valid_token()))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status_of(build_app(build_state(true)), request).await,
            StatusCode::UNAUTHORIZED,
            "{path} must ignore the access_token query param"
        );
    }
}

// -- CORS preflight (handled by the outer CORS layer, never reaches auth) --
