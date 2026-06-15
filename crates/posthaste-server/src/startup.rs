use super::*;

/// Initialize the entire backend (config, store, supervisor, logging)
/// and spawn the Axum server on a Tokio task. Returns immediately.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-accounts#initialization
pub async fn start_server(server_config: ServerConfig) -> ServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let roots = resolve_roots();

    let settings_repo =
        TomlConfigRepository::open(&roots.config_root).expect("failed to open config directory");

    let runtime =
        config::read_daemon_settings(&settings_repo).expect("failed to read runtime settings");
    let config_was_empty = settings_repo.is_empty();
    drop(settings_repo);

    let log_guard = logging::init(&roots.state_root, &runtime.log_level);

    // MIGRATION(api-runtime-wrapper): authority runtime owns config/store/
    // service/secret/event assembly; AppState temporarily exposes those parts
    // to existing Axum handlers through api_bridge.
    //
    // @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#server-startup-authority-builder
    let runtime_build = build_authority_runtime(
        AuthorityRuntimeBuildConfig::new(
            roots.config_root.clone(),
            roots.state_root.clone(),
            roots.state_root.join("cache"),
        )
        .with_bootstrap_path_option(roots.bootstrap_path.clone())
        .with_poll_interval(Duration::from_secs(runtime.poll_interval_seconds)),
    )
    .await
    .expect("failed to build authority runtime");

    if config_was_empty {
        if let Some(bootstrap_path) = &roots.bootstrap_path {
            ph_info!(
                events::CONFIG_BOOTSTRAP_IMPORTED,
                path = %bootstrap_path.display(),
                "imported bootstrap template"
            );
        } else {
            ph_info!(
                events::CONFIG_DEFAULT_INITIALIZED,
                "initialized default config"
            );
        }
    }

    let service = runtime_build.api_bridge.service.clone();
    let store = runtime_build.api_bridge.store.clone();
    let secret_store = runtime_build.api_bridge.secret_store.clone();
    let event_sender = runtime_build.api_bridge.event_sender.clone();
    let supervisor = runtime_build.account_supervisor.clone();

    // Per-process bearer token for the loopback trust model: a full-scope
    // macaroon (no caveats) minted from the per-install root key. Replaces the
    // former random uuid token — opaque to all clients, accepted on every
    // request exactly as before. Always generated (exposed via the handle +
    // daemon.json) but only enforced when `require_auth` is on. The root key is
    // resolved (env → keyring → 0600 state-dir file, generating on first run)
    // so the server runs headless where no keyring exists.
    let macaroon_root_key = token::resolve_root_key(secret_store.as_ref(), &roots.state_root);
    let auth_token = token::mint_full_scope_token(&macaroon_root_key);
    let origin_allowlist =
        auth::origin_allowlist(&runtime.cors_origin, &server_config.extra_cors_origins);

    // Resolve the bind address up front so the Host allowlist can include the
    // configured bind host. (The same value is used to bind the listener below.)
    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| runtime.bind_address.clone());
    let host_allowlist = auth::host_allowlist(&bind_address);

    let state = Arc::new(AppState {
        runtime: runtime_build.handle.clone(),
        service,
        store,
        secret_store,
        supervisor,
        event_sender,
        account_logo_root: roots.config_root.join("account-assets").join("logos"),
        oauth_flows: Arc::new(OAuthFlowStore::default()),
        auth_token: auth_token.clone(),
        macaroon_root_key,
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
            // SECURITY: log `uri().path()` only — never `uri()` (the query
            // string can carry sensitive values) and never the `Authorization`
            // header (the bearer token). Keep this span free of any
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

    // Routes + auth middleware live in `build_api_router` (shared with tests);
    // the runtime-only tracing + CORS layers wrap it here. Layer order is
    // preserved: cors (outermost) → trace → auth → routes.
    let api = build_api_router(state).layer(trace_layer).layer(cors);

    // Browsable API docs at /v1/docs, backed by the spec served at
    // /v1/openapi.json. Swagger UI bundles its own assets (works offline).
    let api_docs = utoipa_swagger_ui::SwaggerUi::new("/v1/docs")
        .config(utoipa_swagger_ui::Config::new(["/v1/openapi.json"]));

    // Bind before assembling the app so the browser-serve fallback can inject
    // the actual bound port (the override may request an OS-assigned `:0`).
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind server listener");
    let addr = listener.local_addr().expect("failed to get local address");

    let app = if let Some(frontend_dist) = server_config.frontend_dist.clone() {
        // SPA fallback: static assets via ServeDir; any unmatched path returns
        // `index.html` with the auth token + port injected so the browser app
        // can authenticate under `require_auth`. The embedded Tauri app uses
        // `frontend_dist = None` and never reaches this handler (it injects the
        // token via the webview init script instead).
        Router::new()
            .nest("/v1", api)
            .fallback_service(spa_fallback_service(
                &frontend_dist,
                &auth_token,
                addr.port(),
            ))
    } else {
        Router::new().nest("/v1", api)
    }
    .merge(api_docs);

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
        runtime_shutdown: runtime_build.shutdown,
        auth_token,
        require_auth: runtime.require_auth,
    }
}
