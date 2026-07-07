use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::{get, patch, post};
use axum::{middleware, Router};
use posthaste_authority_server::AccountSupervisor;
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{SecretRef, SecretStoreError};
use posthaste_domain_service::{ConfigRepository, MailService, MailStore, SecretStore};
use posthaste_http_api_adapter::auth::require_auth_layer;
use posthaste_http_api_adapter::token::{mint_full_scope_token, RootKey};
use posthaste_http_api_adapter::AppState;
use posthaste_store::DatabaseStore;
use posthaste_testkit::temp_root;
use tokio::sync::broadcast;
use tower::ServiceExt;

const CORS_ORIGIN: &str = "http://localhost:5173";

pub(super) fn test_root_key() -> RootKey {
    RootKey::from_test_bytes([42u8; 32])
}

pub(super) fn full_scope() -> String {
    mint_full_scope_token(&test_root_key())
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

/// A `200 OK` stand-in for any protected endpoint.
async fn ok() -> StatusCode {
    StatusCode::OK
}

pub(super) fn build_state() -> Arc<AppState> {
    let root = temp_root("posthaste-capability-scoping-test");
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
        runtime: posthaste_testkit::runtime_handle_with_account_runtime_provider_for_migration(
            service,
            store.clone(),
            secret_store.clone(),
            event_sender,
            supervisor,
        ),
        account_logo_root: state_root.join("account-assets/logos"),
        config_root: state_root.clone(),
        auth_token: full_scope(),
        macaroon_root_key: test_root_key(),
        require_auth: true,
        origin_allowlist: posthaste_http_api_adapter::auth::origin_allowlist(CORS_ORIGIN, &[]),
        host_allowlist: posthaste_http_api_adapter::auth::host_allowlist("127.0.0.1:3001"),
    })
}

/// A `/v1`-nested router carrying the auth layer, with the real route templates
/// for the endpoints exercised here (so `MatchedPath` and the authz lookup
/// behave exactly as in production). All handlers return 200; the test cares
/// only about whether the middleware allows or rejects the request.
pub(super) fn build_app(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/accounts", get(ok))
        .route("/read", post(ok))
        .route("/views/conversations", get(ok))
        .route("/views", post(ok))
        .route("/views/{view_id}/stream", get(ok))
        .route("/runtime/sessions", post(ok))
        .route("/runtime/sessions/{session_id}", axum::routing::delete(ok))
        .route("/runtime/sessions/{session_id}/stream", get(ok))
        .route("/runtime/sessions/{session_id}/views", post(ok))
        .route(
            "/runtime/sessions/{session_id}/views/{view_id}",
            axum::routing::delete(ok),
        )
        .route("/runtime/sessions/{session_id}/mutations", post(ok))
        .route("/smart-mailboxes/{smart_mailbox_id}/conversations", get(ok))
        .route("/events", get(ok))
        .route("/sources/{source_id}/messages", get(ok))
        .route("/sources/{source_id}/messages/{message_id}", get(ok))
        .route("/sources/{source_id}/commands/send", post(ok))
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/set-keywords",
            post(ok),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
            post(ok),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/destroy",
            post(ok),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/unsubscribe",
            post(ok),
        )
        .route("/settings", patch(ok))
        .route("/config:reload", post(ok))
        .route("/auth/tokens", post(ok))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_layer,
        ))
        .with_state(state);
    Router::new().nest("/v1", api)
}

pub(super) fn request(method: &str, path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build")
}

pub(super) async fn status(token: &str, method: &str, path: &str) -> StatusCode {
    let app = build_app(build_state());
    app.oneshot(request(method, path, token))
        .await
        .expect("router should respond")
        .status()
}

// -- Full-scope token: no regression across every verb class. --
