//! Integration tests for the loopback trust-model middleware.
//!
//! The headline invariant: with `require_auth` off (the default), an
//! un-tokened request still succeeds, so shipped behavior is byte-identical.
//! With it on, the bearer token + Origin/Host guard are enforced, with the
//! documented exemptions.
//!
//! @spec docs/eph/DESIGN-L1-trust-model

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::{middleware, Router};
use http_body_util::BodyExt;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    ConfigRepository, MailService, MailStore, SecretRef, SecretStore, SecretStoreError,
};
use posthaste_server::auth::require_auth_layer;
use posthaste_server::supervisor::AccountSupervisor;
use posthaste_server::token::{mint_full_scope_token, RootKey};
use posthaste_server::AppState;
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;
use tower::ServiceExt;

const CORS_ORIGIN: &str = "http://localhost:5173";

/// Fixed 32-byte test root key, so the minted macaroon and the verifier in the
/// auth middleware share a key without touching the env or a keyring.
fn test_root_key() -> RootKey {
    RootKey::from_test_bytes([42u8; 32])
}

/// A valid full-scope macaroon minted under the test root key.
fn valid_token() -> String {
    mint_full_scope_token(&test_root_key())
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-auth-middleware-test-{now}-{seq}"))
}

struct TestSecretStore;

impl SecretStore for TestSecretStore {
    fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        Err(SecretStoreError::Unavailable("unused".to_string()))
    }
    fn save(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
    fn update(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
    fn delete(&self, _secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
}

/// A `200 OK` handler standing in for any protected `/v1` endpoint.
async fn protected() -> StatusCode {
    StatusCode::OK
}

/// Liveness handler standing in for `GET /v1/health`.
async fn health() -> StatusCode {
    StatusCode::OK
}

/// Build the app state with the auth flag toggled, mirroring `start_server`.
fn build_state(require_auth: bool) -> Arc<AppState> {
    let root = temp_root();
    let config_root = root.join("config");
    let state_root = root.join("state");
    let config_repo =
        TomlConfigRepository::open(&config_root).expect("config repository should open");
    config_repo
        .initialize_defaults()
        .expect("config defaults should initialize");
    let database_store = Arc::new(
        DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
            .expect("database store should open"),
    );
    let store: Arc<dyn MailStore> = database_store.clone();
    let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
    let service = Arc::new(MailService::new(database_store, config));
    let (event_sender, _) = broadcast::channel(16);
    let secret_store: Arc<dyn SecretStore> = Arc::new(TestSecretStore);
    let supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        Duration::from_secs(60),
    ));
    Arc::new(AppState {
        service,
        store,
        secret_store,
        supervisor,
        event_sender,
        account_logo_root: state_root.join("account-assets/logos"),
        oauth_flows: Arc::new(posthaste_server::oauth::OAuthFlowStore::default()),
        auth_token: valid_token(),
        macaroon_root_key: test_root_key(),
        require_auth,
        origin_allowlist: posthaste_server::auth::origin_allowlist(
            CORS_ORIGIN,
            &[
                "tauri://localhost".to_string(),
                "https://tauri.localhost".to_string(),
            ],
        ),
        host_allowlist: posthaste_server::auth::host_allowlist("127.0.0.1:3001"),
    })
}

/// Build a `/v1`-nested router carrying the auth layer, exactly as the real
/// server wires it (so the middleware sees nest-stripped paths).
fn build_app(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(protected))
        .route("/settings", get(protected))
        .route("/events", get(protected))
        .route("/account-assets/logos/{image_id}", get(protected))
        .route(
            "/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
            get(protected),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_layer,
        ))
        .with_state(state);
    Router::new().nest("/v1", api)
}

async fn status_of(app: Router, request: Request<Body>) -> StatusCode {
    app.oneshot(request)
        .await
        .expect("router should respond")
        .status()
}

/// Build a `GET` request with a default allowlisted `Host` header. Tests that
/// exercise the Host gate override it with `.header(header::HOST, ...)`, which
/// replaces this default.
fn get_request(path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("GET")
        .uri(path)
        .header(header::HOST, "127.0.0.1")
}

// -- Safety invariant: flag off --

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

/// Builder with NO default Host header, for Host-gate tests.
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
