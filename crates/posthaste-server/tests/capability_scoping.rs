//! Stage B integration tests: ATTENUATED macaroons (carrying first-party
//! caveats) actually restrict access. Built on real route templates so the auth
//! middleware resolves a `MatchedPath` and looks up the authz map exactly as in
//! production. A fixed test root key lets the test mint + attenuate real tokens.
//!
//! The 401-vs-403 split is load-bearing and asserted throughout: a forged token
//! is 401 (Unauthorized); an authentic token whose caveats are out of scope is
//! 403 (Forbidden).
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::{get, patch, post};
use axum::{middleware, Router};
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    ConfigRepository, MailService, MailStore, SecretRef, SecretStore, SecretStoreError,
};
use posthaste_server::auth::require_auth_layer;
use posthaste_server::supervisor::AccountSupervisor;
use posthaste_server::token::{attenuate, mint_full_scope_token, mint_with_caveats, RootKey};
use posthaste_server::AppState;
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;
use tower::ServiceExt;

const CORS_ORIGIN: &str = "http://localhost:5173";

fn test_root_key() -> RootKey {
    RootKey::from_test_bytes([42u8; 32])
}

fn full_scope() -> String {
    mint_full_scope_token(&test_root_key())
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-capability-scoping-test-{now}-{seq}"))
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

fn build_state() -> Arc<AppState> {
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
        auth_token: full_scope(),
        macaroon_root_key: test_root_key(),
        require_auth: true,
        origin_allowlist: posthaste_server::auth::origin_allowlist(CORS_ORIGIN, &[]),
        host_allowlist: posthaste_server::auth::host_allowlist("127.0.0.1:3001"),
    })
}

/// A `/v1`-nested router carrying the auth layer, with the real route templates
/// for the endpoints exercised here (so `MatchedPath` and the authz lookup
/// behave exactly as in production). All handlers return 200; the test cares
/// only about whether the middleware allows or rejects the request.
fn build_app(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/accounts", get(ok))
        .route("/sidebar", get(ok))
        .route("/views/conversations", get(ok))
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
        .route("/settings", patch(ok))
        .route("/config:reload", post(ok))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_layer,
        ))
        .with_state(state);
    Router::new().nest("/v1", api)
}

fn request(method: &str, path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build")
}

async fn status(token: &str, method: &str, path: &str) -> StatusCode {
    let app = build_app(build_state());
    app.oneshot(request(method, path, token))
        .await
        .expect("router should respond")
        .status()
}

// -- Full-scope token: no regression across every verb class. --

#[tokio::test]
async fn full_scope_token_allows_all_verb_classes() {
    let t = full_scope();
    assert_eq!(status(&t, "GET", "/v1/accounts").await, StatusCode::OK);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/add-to-mailbox"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/destroy"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "POST", "/v1/config:reload").await,
        StatusCode::OK
    );
}

// -- action caveat. --

#[tokio::test]
async fn read_only_token_allows_get_denies_writes() {
    let t = attenuate(&full_scope(), "action = read").unwrap();
    // A read passes.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    // Send (Send action) is out of scope → 403.
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::FORBIDDEN
    );
    // Tag and destroy are also denied.
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/destroy"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn multi_action_token_allows_each_listed_verb() {
    let t = attenuate(&full_scope(), "action = read,tag").unwrap();
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::OK
    );
    // move is not listed → denied.
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/add-to-mailbox"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

// -- account caveat (Gate routes). --

#[tokio::test]
async fn account_token_gates_by_path_source() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-b/messages/m1").await,
        StatusCode::FORBIDDEN
    );
    // The account list has no scopable account axis → an account-scoped token
    // is rejected there.
    assert_eq!(
        status(&t, "GET", "/v1/accounts").await,
        StatusCode::FORBIDDEN
    );
}

// -- message caveat. --

#[tokio::test]
async fn message_token_gates_by_path_message() {
    let t = mint_with_caveats(&test_root_key(), &["message = m1"]);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m2").await,
        StatusCode::FORBIDDEN
    );
}

// -- expires caveat. --

#[tokio::test]
async fn expired_token_is_forbidden_future_is_allowed() {
    let past = attenuate(&full_scope(), "expires = 2020-01-01T00:00:00Z").unwrap();
    assert_eq!(
        status(&past, "GET", "/v1/accounts").await,
        StatusCode::FORBIDDEN
    );
    let future = attenuate(&full_scope(), "expires = 2099-01-01T00:00:00Z").unwrap();
    assert_eq!(status(&future, "GET", "/v1/accounts").await, StatusCode::OK);
}

// -- Filter route still backed by a result-side-filtered handler: GET /events
//    (keyed on accountId). A matching filter satisfies the caveat; a missing or
//    non-matching one denies. --

#[tokio::test]
async fn events_filter_route_requires_matching_account_filter() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    // Matching filter → allowed.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-a").await,
        StatusCode::OK
    );
    // No filter → the account axis is None → unsatisfiable → 403.
    assert_eq!(status(&t, "GET", "/v1/events").await, StatusCode::FORBIDDEN);
    // Non-matching filter → 403.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-b").await,
        StatusCode::FORBIDDEN
    );
}

// -- SECURITY regression: the conversation-list routes are GLOBAL reads in
//    Phase 1 (their handlers do not result-side filter by source/mailbox in
//    every branch). An account-scoped token must be DENIED on them even WITH a
//    matching `sourceId` filter — otherwise an `account=X` token would read all
//    accounts' conversations. This is the leak closure: assert 403, not 200. --

#[tokio::test]
async fn account_token_denied_on_conversation_list_even_with_matching_filter() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);

    // /views/conversations: the `q` search branch drops the source/mailbox
    // filter, so the route is a global read; account caveat unsatisfiable → 403.
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-a").await,
        StatusCode::FORBIDDEN,
        "account-scoped token must be denied on /views/conversations (handler does \
         not enforce source filter in the search branch)"
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::FORBIDDEN
    );

    // /smart-mailboxes/{id}/conversations: handler never filters by
    // source/mailbox in any branch → global read; account caveat → 403.
    assert_eq!(
        status(
            &t,
            "GET",
            "/v1/smart-mailboxes/sm-1/conversations?sourceId=acct-a"
        )
        .await,
        StatusCode::FORBIDDEN,
        "account-scoped token must be denied on smart-mailbox conversations \
         (handler ignores the source filter entirely)"
    );
}

#[tokio::test]
async fn full_scope_token_still_reads_conversation_lists() {
    // No regression: a full-scope token (no caveats, fast path) still works on
    // the conversation lists, with or without a filter.
    let t = full_scope();
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-a").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/smart-mailboxes/sm-1/conversations").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn read_only_token_reads_conversation_lists() {
    // An `action = read` token (no resource caveat) is also fine — the routes
    // are Read, and there is no account/mailbox caveat to be unsatisfiable.
    let t = attenuate(&full_scope(), "action = read").unwrap();
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/smart-mailboxes/sm-1/conversations").await,
        StatusCode::OK
    );
}

// -- Low finding: duplicate query key fails closed. A Filter route that still
//    declares a query axis (/events on accountId) must DENY when the key appears
//    twice, rather than first-wins authorizing `?accountId=a&accountId=b`. --

#[tokio::test]
async fn duplicate_filter_param_is_denied() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    // First-wins would have matched `acct-a` and allowed; we fail closed → 403.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-a&accountId=acct-b").await,
        StatusCode::FORBIDDEN,
        "a duplicated filter key must fail closed (deny), not take the first value"
    );
    // Order-independent: duplicate is denied even if the matching value is last.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-b&accountId=acct-a").await,
        StatusCode::FORBIDDEN
    );
}

// -- Global route + scoped token. --

#[tokio::test]
async fn scoped_token_on_global_route_is_forbidden() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    assert_eq!(
        status(&t, "GET", "/v1/sidebar").await,
        StatusCode::FORBIDDEN
    );
}

// -- 401 vs 403 split: a forged token is 401, not 403. --

#[tokio::test]
async fn forged_macaroon_is_unauthorized_not_forbidden() {
    // A well-formed macaroon under a DIFFERENT root key fails authenticity → 401.
    let foreign = mint_with_caveats(&RootKey::from_test_bytes([1u8; 32]), &["action = read"]);
    assert_eq!(
        status(&foreign, "GET", "/v1/accounts").await,
        StatusCode::UNAUTHORIZED
    );
    // Garbage is also 401.
    assert_eq!(
        status("not-a-macaroon", "GET", "/v1/accounts").await,
        StatusCode::UNAUTHORIZED
    );
}

// -- combined caveats AND together. --

#[tokio::test]
async fn combined_account_and_action_caveats_and_together() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a", "action = read"]);
    // In-scope account + read → allowed.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    // Right account, wrong action (send) → denied.
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::FORBIDDEN
    );
    // Right action (read), wrong account → denied.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-b/messages/m1").await,
        StatusCode::FORBIDDEN
    );
}
