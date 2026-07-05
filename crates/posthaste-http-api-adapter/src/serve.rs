//! The near serving glue: build the `/v1` app state, then bind + serve the
//! composed router. Shared by the bundled `posthaste-server` start path and the
//! lean remote runtime daemon, which differ only in how they build the runtime
//! and which extra routers (OAuth, link) they layer on.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use posthaste_domain_service::SecretStore;
use posthaste_observability::{events, fail_closed, ph_error, ph_info};
use posthaste_runtime::{RuntimeBuildConfig, RuntimeHandle, RuntimeShutdownHandle};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::ResolvedRoots;
use crate::shutdown::{StoreClose, SupervisorStop};
use crate::{auth, logging, observability, token, AppState, ServerConfig, ServerHandle};
use posthaste_config::DaemonSettings;

/// The shared node-assembly preamble (RFC D27): resolve roots, read daemon
/// settings (`app.toml` + `POSTHASTE_*` env), init logging, and build the base
/// [`RuntimeBuildConfig`] (config/state/cache roots + poll interval).
///
/// This dedupes the preamble that was triplicated verbatim across
/// `posthaste-server`'s `startup.rs` + `startup_authority_server.rs` and
/// `posthaste-runtimed`'s `main.rs`. Each caller extends `build_config` with its
/// role-specific transport/bootstrap and uses `daemon`/`roots`/`log_guard` for the
/// rest of startup. Settings are read via `TomlConfigRepository` so the bundled
/// server can observe `config_was_empty` for its bootstrap-import logging.
pub fn assemble_daemon_preamble() -> DaemonPreamble {
    let roots = crate::resolve_roots();
    let settings_repo = posthaste_config::TomlConfigRepository::open(&roots.config_root)
        .expect("failed to open config directory");
    let daemon = posthaste_config::read_daemon_settings(&settings_repo)
        .expect("failed to read runtime settings");
    let config_was_empty = settings_repo.is_empty();
    drop(settings_repo);
    let log_guard = logging::init(&roots.state_root, &daemon.log_level);
    let build_config = RuntimeBuildConfig::new(
        roots.config_root.clone(),
        roots.state_root.clone(),
        roots.state_root.join("cache"),
    )
    .with_poll_interval(Duration::from_secs(daemon.poll_interval_seconds));
    DaemonPreamble {
        roots,
        daemon,
        log_guard,
        build_config,
        config_was_empty,
    }
}

/// The resolved inputs every role binary shares at the top of `main`/`start_*`:
/// roots, daemon settings, the logging guard, the base `RuntimeBuildConfig`, and
/// whether the config directory was empty (for the bundled server's
/// bootstrap-import logging). See [`assemble_daemon_preamble`].
pub struct DaemonPreamble {
    pub roots: ResolvedRoots,
    pub daemon: DaemonSettings,
    pub log_guard: WorkerGuard,
    pub build_config: RuntimeBuildConfig,
    pub config_was_empty: bool,
}

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
        config_root: roots.config_root.clone(),
        auth_token,
        macaroon_root_key,
        require_auth: daemon.require_auth,
        origin_allowlist,
        host_allowlist,
    })
}

/// Inputs for [`serve`]: the composed `/v1` router (the API router, optionally
/// merged with the far OAuth router), any extra root-level routers to merge (the
/// runtime↔authority-server `link_router`, which registers absolute `/v1/link/*` paths),
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
    /// The shared cancellation token: attached to axum's graceful shutdown here
    /// and carried onto the [`ServerHandle`] for the composition root's
    /// [`crate::ShutdownSequence`].
    pub shutdown_token: CancellationToken,
    /// Teardown step (b) supervisor seam; `None` for a lean near node.
    pub supervisor_stop: Option<Box<dyn SupervisorStop>>,
    /// Teardown step (c) store seam; `None` for a lean near node.
    pub store_close: Option<Box<dyn StoreClose>>,
    /// Optional in-daemon TLS; present ⇒ serve HTTPS via `crate::tls`.
    pub tls: Option<posthaste_config::TlsConfig>,
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
        shutdown_token,
        supervisor_stop,
        store_close,
        tls,
    } = opts;

    // Build the TLS acceptor up front so an invalid [tls] config fails fast at
    // startup (before the listener is announced), not on the first connection.
    let tls_acceptor = tls.as_ref().map(|config| {
        // Deliberate fail-closed (D73): refuse to serve rather than silently fall
        // back to plaintext on an invalid [tls] config.
        crate::tls::build_tls_acceptor(config)
            .unwrap_or_else(|err| fail_closed!("invalid [tls] configuration: {err}"))
    });

    let origins: Vec<axum::http::HeaderValue> = cors_origins
        .iter()
        .map(|origin| {
            // Deliberate fail-closed (D73): a malformed CORS origin must abort
            // startup, never widen the allowlist by silently dropping an entry.
            origin
                .parse()
                .unwrap_or_else(|err| fail_closed!("invalid CORS origin {origin:?}: {err}"))
        })
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
                process_role = "authority-server",
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
        )
        // D72: no 5xx leaves the boundary operator-invisible. The default
        // `ServerErrorsAsFailures` classifier fires this on every 5xx; the log
        // inherits the span's `request_id`, and the sanitized construction-time
        // `HTTP_INTERNAL_ERROR` log (in the same span) carries the real cause +
        // the correlation id echoed to the client. Together they join a 500 body
        // to its cause.
        .on_failure(
            |failure: tower_http::classify::ServerErrorsFailureClass,
             latency: Duration,
             span: &Span| {
                ph_error!(
                    parent: span,
                    events::HTTP_INTERNAL_ERROR,
                    failure = %failure,
                    latency_ms = latency.as_millis() as u64,
                    "http request failed at the /v1 boundary"
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

    // Graceful shutdown (D60 phase (a)): cancelling the shared token makes axum
    // stop accepting and drain in-flight requests/SSE before the serve future
    // returns. The `ShutdownSequence` owns the cancel; here we only wire the
    // drain. One owned wait-future per arm (only one arm runs).
    let drain_token = shutdown_token.clone();
    let join_handle = tokio::spawn(async move {
        match tls_acceptor {
            Some(acceptor) => {
                let tls_listener = crate::tls::TlsListener::new(listener, acceptor);
                axum::serve(tls_listener, app)
                    .with_graceful_shutdown(drain_token.cancelled_owned())
                    .await
                    .expect("posthaste server failed");
            }
            None => {
                axum::serve(listener, app)
                    .with_graceful_shutdown(drain_token.cancelled_owned())
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
        shutdown_token,
        supervisor_stop,
        store_close,
        auth_token,
        require_auth,
    }
}
