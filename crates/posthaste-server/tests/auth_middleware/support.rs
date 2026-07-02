use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::{middleware, Router};
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{SecretRef, SecretStoreError};
use posthaste_domain_service::{ConfigRepository, MailService, MailStore, SecretStore};
use posthaste_http_api_adapter::auth::require_auth_layer;
use posthaste_authority_server::AccountSupervisor;
use posthaste_http_api_adapter::token::{mint_full_scope_token, RootKey};
use posthaste_http_api_adapter::AppState;
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;
use tower::ServiceExt;

pub(super) const CORS_ORIGIN: &str = "http://localhost:5173";

/// Fixed 32-byte test root key, so the minted macaroon and the verifier in the
/// auth middleware share a key without touching the env or a keyring.
pub(super) fn test_root_key() -> RootKey {
    RootKey::from_test_bytes([42u8; 32])
}

/// A valid full-scope macaroon minted under the test root key.
pub(super) fn valid_token() -> String {
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
pub(super) async fn protected() -> StatusCode {
    StatusCode::OK
}

/// Liveness handler standing in for `GET /v1/health`.
async fn health() -> StatusCode {
    StatusCode::OK
}

/// Build the app state with the auth flag toggled, mirroring `start_server`.
pub(super) fn build_state(require_auth: bool) -> Arc<AppState> {
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
        runtime: posthaste_testkit::runtime_handle_with_account_runtime_provider_for_migration(
            service,
            store.clone(),
            secret_store.clone(),
            event_sender,
            supervisor,
        ),
        account_logo_root: state_root.join("account-assets/logos"),
        auth_token: valid_token(),
        macaroon_root_key: test_root_key(),
        require_auth,
        origin_allowlist: posthaste_http_api_adapter::auth::origin_allowlist(
            CORS_ORIGIN,
            &[
                "tauri://localhost".to_string(),
                "https://tauri.localhost".to_string(),
            ],
        ),
        host_allowlist: posthaste_http_api_adapter::auth::host_allowlist("127.0.0.1:3001"),
    })
}

/// Build a `/v1`-nested router carrying the auth layer, exactly as the real
/// server wires it (so the middleware sees nest-stripped paths).
pub(super) fn build_app(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(protected))
        .route("/oauth/callback", get(protected))
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

pub(super) async fn status_of(app: Router, request: Request<Body>) -> StatusCode {
    app.oneshot(request)
        .await
        .expect("router should respond")
        .status()
}

/// Build a `GET` request with a default allowlisted `Host` header. Tests that
/// exercise the Host gate override it with `.header(header::HOST, ...)`, which
/// replaces this default.
pub(super) fn get_request(path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("GET")
        .uri(path)
        .header(header::HOST, "127.0.0.1")
}

// -- Safety invariant: flag off --
