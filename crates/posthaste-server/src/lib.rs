pub mod api;
pub mod config;
pub mod logging;
pub mod oauth;
pub mod observability;
pub mod push;
pub mod sanitize;
pub mod secret;
pub mod supervisor;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::Router;
#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{ConfigRepository, DomainEvent, MailService, MailStore, SecretStore};
use posthaste_observability::{events, ph_info, ph_warn};
use posthaste_store::DatabaseStore;
use posthaste_telemetry::{
    upload_pending, TelemetryEvent, TelemetryResult, TelemetrySpool, UploadConfig,
};
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{resolve_roots, DaemonSettings};
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
    pub telemetry_root: PathBuf,
    pub oauth_flows: Arc<OAuthFlowStore>,
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

fn spawn_telemetry_upload_worker(
    telemetry_root: PathBuf,
    service: Arc<MailService>,
    runtime: &DaemonSettings,
) {
    let Some(endpoint) = runtime.telemetry_endpoint.clone() else {
        return;
    };
    let config = UploadConfig {
        endpoint,
        ingest_token: runtime.telemetry_ingest_token.clone(),
    };
    let interval_seconds = runtime.telemetry_upload_interval_seconds.max(60);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
        loop {
            interval.tick().await;
            let Ok(settings) = service.get_app_settings() else {
                continue;
            };
            if matches!(
                settings.telemetry.mode,
                posthaste_domain::TelemetryMode::Off
            ) {
                continue;
            }
            match upload_pending(&telemetry_root, &config, &client).await {
                Ok(outcome) if outcome.uploaded > 0 || outcome.discarded > 0 => {
                    ph_info!(
                        events::TELEMETRY_UPLOAD_COMPLETED,
                        uploaded_count = outcome.uploaded,
                        retained_count = outcome.retained,
                        discarded_count = outcome.discarded,
                        "uploaded telemetry batches"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    ph_warn!(
                        events::TELEMETRY_UPLOAD_FAILED,
                        error = %error,
                        "failed to upload telemetry batches"
                    );
                }
            }
        }
    });
}

/// Handle returned by [`start_server`]. Holds the bound address, the server
/// task, and the log guard that must survive for the process lifetime.
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
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

/// Initialize the entire backend (config, store, supervisor, logging)
/// and spawn the Axum server on a Tokio task. Returns immediately.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-accounts#initialization
pub async fn start_server(server_config: ServerConfig) -> ServerHandle {
    let startup_started_at = Instant::now();
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

    let telemetry_settings = service
        .get_app_settings()
        .expect("failed to load app settings")
        .telemetry;
    let telemetry_spool = TelemetrySpool::new(
        &roots.state_root,
        telemetry_settings,
        env!("CARGO_PKG_VERSION").to_string(),
    );
    if let Err(error) = telemetry_spool.emit(TelemetryEvent::app_startup_completed(
        startup_started_at.elapsed(),
        TelemetryResult::Ok,
    )) {
        ph_warn!(
            events::TELEMETRY_SPOOL_WRITE_FAILED,
            error = %error,
            "failed to spool telemetry event"
        );
    }
    spawn_telemetry_upload_worker(
        roots.state_root.join("telemetry"),
        service.clone(),
        &runtime,
    );

    let state = Arc::new(AppState {
        service,
        store,
        secret_store,
        supervisor,
        event_sender,
        account_logo_root: roots.config_root.join("account-assets").join("logos"),
        telemetry_root: roots.state_root.join("telemetry"),
        oauth_flows: Arc::new(OAuthFlowStore::default()),
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
        .layer(trace_layer)
        .layer(cors)
        .with_state(state);

    let app = if let Some(frontend_dist) = server_config.frontend_dist.clone() {
        Router::new().nest("/v1", api).fallback_service(
            ServeDir::new(&frontend_dist)
                .fallback(ServeFile::new(frontend_dist.join("index.html"))),
        )
    } else {
        Router::new().nest("/v1", api)
    };

    let bind_address = server_config
        .bind_address_override
        .as_deref()
        .unwrap_or(&runtime.bind_address);
    let listener = tokio::net::TcpListener::bind(bind_address)
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
    }
}
