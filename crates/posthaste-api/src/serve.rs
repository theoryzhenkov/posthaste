//! The near serving glue: build the `/v1` app state, then bind + serve the
//! composed router. Shared by the bundled `posthaste-server` start path and the
//! lean remote runtime daemon, which differ only in how they build the runtime
//! and which extra routers (OAuth, link) they layer on.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use posthaste_domain::SecretStore;
use posthaste_observability::{events, ph_info};
use posthaste_runtime::{RuntimeHandle, RuntimeShutdownHandle};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{DaemonSettings, ResolvedRoots};
use crate::{auth, observability, token, AppState, ServerConfig, ServerHandle};

/// Build the near `/v1` application state from a runtime handle + resolved
/// config: mint the full-scope macaroon, resolve the auth root key, and compute
/// the Origin/Host allowlists. The OAuth fields live in the far `OAuthState`, not
/// here — a lean near node has no provider machinery.
pub fn build_app_state(
    runtime: RuntimeHandle,
    secret_store: &Arc<dyn SecretStore>,
    roots: &ResolvedRoots,
    daemon: &DaemonSettings,
    server_config: &ServerConfig,
) -> Arc<AppState> {
    // Per-process bearer token for the loopback trust model: a full-scope
    // macaroon (no caveats) minted from the per-install root key, resolved
    // (env → keyring → 0600 state-dir file) so the server runs headless.
    let macaroon_root_key = token::resolve_root_key(secret_store.as_ref(), &roots.state_root);
    let auth_token = token::mint_full_scope_token(&macaroon_root_key);
    let origin_allowlist =
        auth::origin_allowlist(&daemon.cors_origin, &server_config.extra_cors_origins);
    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| daemon.bind_address.clone());
    let mut host_allowlist = auth::host_allowlist(&bind_address);
    host_allowlist.extend(daemon.allowed_hosts.iter().cloned());

    Arc::new(AppState {
        runtime,
        account_logo_root: roots.config_root.join("account-assets").join("logos"),
        auth_token,
        macaroon_root_key,
        require_auth: daemon.require_auth,
        origin_allowlist,
        host_allowlist,
    })
}

/// Inputs for [`serve`]: the composed `/v1` router (the API router, optionally
/// merged with the far OAuth router), any extra root-level routers to merge (the
/// runtime↔backend `link_router`, which registers absolute `/v1/link/*` paths),
/// and the serving parameters.
pub struct ServeOptions {
    pub v1_router: Router,
    pub root_merges: Vec<Router>,
    pub bind_address: String,
    /// Browser origins allowed by CORS (the configured origin + any extras).
    pub cors_origins: Vec<String>,
    pub frontend_dist: Option<std::path::PathBuf>,
    pub auth_token: String,
    pub require_auth: bool,
    pub config_root_display: String,
    pub log_guard: WorkerGuard,
    pub runtime_shutdown: RuntimeShutdownHandle,
    /// Optional in-daemon TLS; present ⇒ serve HTTPS via `crate::tls`.
    pub tls: Option<crate::config::TlsConfig>,
}

/// Apply the runtime-only outer layers (CORS + request tracing), nest under
/// `/v1`, attach Swagger UI + the SPA fallback, merge any extra root routers,
/// bind, and spawn the Axum server. Returns immediately.
///
/// @spec docs/L0-api#axum
pub async fn serve(opts: ServeOptions) -> ServerHandle {
    let ServeOptions {
        v1_router,
        root_merges,
        bind_address,
        cors_origins,
        frontend_dist,
        auth_token,
        require_auth,
        config_root_display,
        log_guard,
        runtime_shutdown,
        tls,
    } = opts;

    // Build the TLS acceptor up front so an invalid [tls] config fails fast at
    // startup (before the listener is announced), not on the first connection.
    let tls_acceptor = tls
        .as_ref()
        .map(|config| crate::tls::build_tls_acceptor(config).expect("invalid [tls] configuration"));

    let origins: Vec<axum::http::HeaderValue> = cors_origins
        .iter()
        .map(|origin| origin.parse().expect("invalid CORS origin"))
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            let context = observability::RequestLogContext::from_headers(request.headers());
            // SECURITY: log `uri().path()` only — never `uri()` (query strings
            // can carry sensitive values) and never the `Authorization` header.
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

    let api = v1_router.layer(trace_layer).layer(cors);

    let api_docs = utoipa_swagger_ui::SwaggerUi::new("/v1/docs")
        .config(utoipa_swagger_ui::Config::new(["/v1/openapi.json"]));

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind server listener");
    let addr = listener.local_addr().expect("failed to get local address");

    let mut app = if let Some(frontend_dist) = frontend_dist {
        Router::new()
            .nest("/v1", api)
            .fallback_service(crate::spa_fallback_service(
                &frontend_dist,
                &auth_token,
                addr.port(),
            ))
    } else {
        Router::new().nest("/v1", api)
    }
    .merge(api_docs);

    for extra in root_merges {
        app = app.merge(extra);
    }

    ph_info!(
        events::SERVER_LISTENING,
        address = %addr,
        tls = tls_acceptor.is_some(),
        config_root = %config_root_display,
        "posthaste listening"
    );

    let join_handle = tokio::spawn(async move {
        match tls_acceptor {
            Some(acceptor) => {
                let tls_listener = crate::tls::TlsListener::new(listener, acceptor);
                axum::serve(tls_listener, app)
                    .await
                    .expect("posthaste server failed");
            }
            None => {
                axum::serve(listener, app)
                    .await
                    .expect("posthaste server failed");
            }
        }
    });

    ServerHandle {
        addr,
        join_handle,
        log_guard,
        runtime_shutdown,
        auth_token,
        require_auth,
    }
}
