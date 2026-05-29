pub mod api;
pub mod auth;
pub mod config;
pub mod logging;
pub mod oauth;
pub mod observability;
pub mod openapi;
pub mod push;
pub mod sanitize;
pub mod secret;
pub mod supervisor;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{middleware, Router};
#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{ConfigRepository, DomainEvent, MailService, MailStore, SecretStore};
use posthaste_observability::{events, ph_info};
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::resolve_roots;
use crate::oauth::OAuthFlowStore;
use crate::secret::SystemSecretStore;
use crate::supervisor::AccountSupervisor;

/// Shared application state threaded through all Axum handlers.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-api#endpoint-table
pub struct AppState {
    pub service: Arc<MailService>,
    pub store: Arc<dyn MailStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub supervisor: Arc<AccountSupervisor>,
    pub event_sender: broadcast::Sender<DomainEvent>,
    pub account_logo_root: PathBuf,
    pub oauth_flows: Arc<OAuthFlowStore>,
    /// Per-process bearer token enforced by the auth middleware when
    /// `require_auth` is on. Always generated; inert while the flag is off.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub auth_token: String,
    /// Whether the `/v1` auth middleware enforces the trust model. Default
    /// `false`, so existing behavior is byte-identical until enabled.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
    /// Allowlisted browser origins (configured CORS origin + Tauri webview).
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub origin_allowlist: Vec<String>,
    /// Allowlisted `Host` header values (loopback names + configured bind host).
    /// The mandatory DNS-rebinding defense; checked on every request when
    /// `require_auth` is on.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub host_allowlist: Vec<String>,
}

impl AppState {
    /// Broadcast domain events to all connected SSE clients.
    ///
    /// @spec docs/L1-api#sse-event-stream
    /// @spec docs/L1-sync#event-propagation
    pub fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }
}

/// Handle returned by [`start_server`]. Holds the bound address, the server
/// task, and the log guard that must survive for the process lifetime.
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
    /// Per-process bearer token, exposed so the embedded host can inject it
    /// into the webview as `window.__POSTHASTE_TOKEN__`.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub auth_token: String,
    /// Whether the trust model is enforced. The daemon entrypoint uses this to
    /// decide whether to persist the token to `daemon.json` (an unused
    /// credential is never written to disk).
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
}

/// Additional origins to allow in CORS beyond the configured default.
#[derive(Default)]
pub struct ServerConfig {
    pub extra_cors_origins: Vec<String>,
    /// Override the configured bind address (e.g. `"127.0.0.1:0"`
    /// for OS-assigned ports in the Tauri shell).
    pub bind_address_override: Option<String>,
    /// Static frontend distribution to serve for browser-localhost mode.
    pub frontend_dist: Option<PathBuf>,
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// Write `contents` to `path`, creating/truncating it with owner-only (`0600`)
/// permissions on unix. Used for the `daemon.json` port-file, which carries a
/// live bearer token. `fs::write` would NOT tighten an already world-readable
/// file, so this opens with restrictive mode and re-asserts `0600` to cover the
/// overwrite case. On non-unix platforms it falls back to a plain write
/// (filesystem ACLs are the protection there).
///
/// @spec docs/eph/DESIGN-L1-trust-model
pub fn write_secure_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `mode` applies only when the file is newly created.
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // Re-assert 0600 because `mode` above is ignored when the file already
    // existed (e.g. a prior, looser-permissioned daemon.json).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)?;
    file.flush()
}

/// Initialize the entire backend (config, store, supervisor, logging)
/// and spawn the Axum server on a Tokio task. Returns immediately.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-accounts#initialization
pub async fn start_server(server_config: ServerConfig) -> ServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let roots = resolve_roots();

    let config_repo =
        TomlConfigRepository::open(&roots.config_root).expect("failed to open config directory");

    let runtime =
        config::read_daemon_settings(&config_repo).expect("failed to read runtime settings");

    let log_guard = logging::init(&roots.state_root, &runtime.log_level);

    if config_repo.is_empty() {
        if let Some(bootstrap_path) = &roots.bootstrap_path {
            config::import_bootstrap(bootstrap_path, &config_repo)
                .expect("failed to import bootstrap template");
            ph_info!(
                events::CONFIG_BOOTSTRAP_IMPORTED,
                path = %bootstrap_path.display(),
                "imported bootstrap template"
            );
        } else {
            config_repo
                .initialize_defaults()
                .expect("failed to initialize default config");
            ph_info!(
                events::CONFIG_DEFAULT_INITIALIZED,
                "initialized default config"
            );
        }
    }

    let db_path = roots.state_root.join("mail.sqlite");
    let database_store = Arc::new(
        DatabaseStore::open(&db_path, &roots.state_root).expect("failed to initialize store"),
    );
    let store: Arc<dyn MailStore> = database_store.clone();

    let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
    let service = Arc::new(MailService::new(database_store.clone(), config.clone()));

    service
        .sync_source_projections()
        .expect("failed to sync source projections");

    let (event_sender, _) = broadcast::channel(512);
    let secret_store: Arc<dyn SecretStore> = Arc::new(SystemSecretStore);
    let supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        Duration::from_secs(runtime.poll_interval_seconds),
    ));

    for source in service
        .list_sources()
        .expect("failed to load source configuration")
    {
        supervisor.start_account(&source).await;
    }

    // Per-process bearer token for the loopback trust model. Always generated
    // (cheap, exposed via the handle + daemon.json) but only enforced when
    // `require_auth` is on. uuid v4 is 122 bits of randomness — adequate for a
    // loopback secret.
    let auth_token = uuid::Uuid::new_v4().to_string();
    let origin_allowlist = auth::origin_allowlist(&runtime.cors_origin);

    // Resolve the bind address up front so the Host allowlist can include the
    // configured bind host. (The same value is used to bind the listener below.)
    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| runtime.bind_address.clone());
    let host_allowlist = auth::host_allowlist(&bind_address);

    let state = Arc::new(AppState {
        service,
        store,
        secret_store,
        supervisor,
        event_sender,
        account_logo_root: roots.config_root.join("account-assets").join("logos"),
        oauth_flows: Arc::new(OAuthFlowStore::default()),
        auth_token: auth_token.clone(),
        require_auth: runtime.require_auth,
        origin_allowlist,
        host_allowlist,
    });

    // Build CORS layer: always include the configured origin, plus any extras.
    let mut origins: Vec<axum::http::HeaderValue> =
        vec![runtime.cors_origin.parse().expect("invalid CORS origin")];
    for extra in &server_config.extra_cors_origins {
        origins.push(extra.parse().expect("invalid extra CORS origin"));
    }
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            let context = observability::RequestLogContext::from_headers(request.headers());
            // SECURITY: log `uri().path()` only — never `uri()` (would include
            // the query string, leaking the SSE `?access_token=` token) and
            // never the `Authorization` header. Keep this span free of any
            // credential-bearing field.
            info_span!(
                "http.request",
                method = %request.method(),
                path = %request.uri().path(),
                request_id = %context.request_id,
                operation_id = context.operation_id.as_deref().unwrap_or(""),
                operation_kind = context.operation_kind.as_deref().unwrap_or(""),
                operation_source = context.operation_source.as_deref().unwrap_or(""),
                session_id = context.session_id.as_deref().unwrap_or(""),
                process_id = std::process::id(),
                process_role = "backend",
                status = field::Empty,
                latency_ms = field::Empty,
            )
        })
        .on_response(
            |response: &axum::http::Response<_>, latency: Duration, span: &Span| {
                let latency_ms = latency.as_millis() as u64;
                span.record("status", response.status().as_u16());
                span.record("latency_ms", latency_ms);
                ph_info!(
                    parent: span,
                    events::HTTP_REQUEST_COMPLETED,
                    status = response.status().as_u16(),
                    latency_ms,
                    "http request completed"
                );
            },
        );

    let api = Router::new()
        .route("/health", get(api::health))
        .route("/openapi.json", get(openapi::openapi_json))
        .route("/asyncapi.json", get(openapi::asyncapi_json))
        .route(
            "/settings",
            get(api::get_settings).patch(api::patch_settings),
        )
        .route(
            "/automation-rules:preview",
            post(api::preview_automation_rule),
        )
        .route(
            "/accounts",
            get(api::list_accounts).post(api::create_account),
        )
        .route(
            "/accounts/{account_id}",
            get(api::get_account)
                .patch(api::patch_account)
                .delete(api::delete_account),
        )
        .route("/accounts/{account_id}/verify", post(api::verify_account))
        .route(
            "/accounts/{account_id}/oauth/start",
            post(api::start_account_oauth),
        )
        .route("/oauth/start", post(api::start_provider_oauth))
        .route("/oauth/callback", get(api::complete_account_oauth))
        .route("/accounts/{account_id}/enable", post(api::enable_account))
        .route("/accounts/{account_id}/disable", post(api::disable_account))
        .route(
            "/accounts/{account_id}/logo",
            post(api::upload_account_logo),
        )
        .route(
            "/account-assets/logos/{image_id}",
            get(api::get_account_logo),
        )
        .route("/sidebar", get(api::get_sidebar))
        .route(
            "/smart-mailboxes",
            get(api::list_smart_mailboxes).post(api::create_smart_mailbox),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}",
            get(api::get_smart_mailbox)
                .patch(api::patch_smart_mailbox)
                .delete(api::delete_smart_mailbox),
        )
        .route(
            "/smart-mailboxes:reset-defaults",
            post(api::reset_default_smart_mailboxes),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}/messages",
            get(api::list_smart_mailbox_messages),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}/conversations",
            get(api::list_smart_mailbox_conversations),
        )
        .route("/views/conversations", get(api::list_conversations))
        .route(
            "/views/conversations/{conversation_id}",
            get(api::get_conversation),
        )
        .route("/sources/{source_id}/mailboxes", get(api::list_mailboxes))
        .route(
            "/sources/{source_id}/mailboxes/{mailbox_id}",
            patch(api::patch_mailbox),
        )
        .route(
            "/sources/{source_id}/messages",
            get(api::list_source_messages),
        )
        .route("/messages/search", get(api::search_messages))
        .route(
            "/sources/{source_id}/messages/{message_id}",
            get(api::get_message),
        )
        .route(
            "/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
            get(api::get_message_attachment),
        )
        .route("/sender-addresses", get(api::list_sender_addresses))
        .route("/sources/{source_id}/identity", get(api::get_identity))
        .route(
            "/sources/{source_id}/messages/{message_id}/reply-context",
            get(api::get_reply_context),
        )
        .route(
            "/sources/{source_id}/commands/send",
            post(api::send_message),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/set-keywords",
            post(api::set_keywords),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
            post(api::add_to_mailbox),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
            post(api::remove_from_mailbox),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
            post(api::replace_mailboxes),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/destroy",
            post(api::destroy_message),
        )
        .route(
            "/sources/{source_id}/commands/sync",
            post(api::trigger_sync),
        )
        .route("/config:reload", post(api::reload_config))
        .route("/events", get(api::stream_events))
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth_layer,
        ))
        .layer(trace_layer)
        .layer(cors)
        .with_state(state);

    // Browsable API docs at /v1/docs, backed by the spec served at
    // /v1/openapi.json. Swagger UI bundles its own assets (works offline).
    let api_docs = utoipa_swagger_ui::SwaggerUi::new("/v1/docs")
        .config(utoipa_swagger_ui::Config::new(["/v1/openapi.json"]));

    let app = if let Some(frontend_dist) = server_config.frontend_dist.clone() {
        Router::new().nest("/v1", api).fallback_service(
            ServeDir::new(&frontend_dist)
                .fallback(ServeFile::new(frontend_dist.join("index.html"))),
        )
    } else {
        Router::new().nest("/v1", api)
    }
    .merge(api_docs);

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind server listener");
    let addr = listener.local_addr().expect("failed to get local address");
    ph_info!(
        events::SERVER_LISTENING,
        address = %addr,
        config_root = %roots.config_root.display(),
        "posthaste listening"
    );

    let join_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("posthaste server failed");
    });

    ServerHandle {
        addr,
        join_handle,
        log_guard,
        auth_token,
        require_auth: runtime.require_auth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn write_secure_file_creates_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "posthaste-secure-file-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should create");
        let path = dir.join("daemon.json");

        write_secure_file(&path, b"{\"port\":1}").expect("secure write should succeed");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "daemon.json must be owner-only");

        // Overwriting a pre-existing (here world-readable) file must re-tighten.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_secure_file(&path, b"{\"port\":2}").expect("secure overwrite should succeed");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "overwrite must tighten back to 0600");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
