use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::{middleware, Router};
use http_body_util::BodyExt;
use posthaste_api::auth::require_auth_layer;
use tower::ServiceExt;

use crate::support::{build_app, build_state, get_request, protected, status_of};

#[tokio::test]
async fn options_preflight_is_handled_by_cors_layer_not_auth() {
    use tower_http::cors::{Any, CorsLayer};

    // Mirror the real server's layer ordering: auth is inner, CORS is the
    // outermost layer, so a preflight OPTIONS is answered by CORS before it can
    // reach the auth middleware (which would otherwise 401/403 it).
    let state = build_state(true);
    let api = Router::new()
        .route("/settings", get(protected))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_layer,
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);
    let app = Router::new().nest("/v1", api);

    let request = Request::builder()
        .method("OPTIONS")
        .uri("/v1/settings")
        .header(header::ORIGIN, "http://evil.example")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();
    // No token, attacker origin, no Host — yet the preflight succeeds because
    // CORS answers it; auth never runs.
    assert_eq!(status_of(app, request).await, StatusCode::OK);
}

// -- Error body shape --

#[tokio::test]
async fn unauthorized_uses_typed_api_error_body() {
    let app = build_app(build_state(true));
    let response = app
        .oneshot(get_request("/v1/settings").body(Body::empty()).unwrap())
        .await
        .expect("router should respond");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    let body: serde_json::Value =
        serde_json::from_slice(&bytes).expect("error body should be JSON");
    assert_eq!(body["code"], "unauthorized");
    assert!(body["message"].is_string());
}
