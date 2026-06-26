use super::*;

use crate::oauth::OAuthFlowStore;

/// Initialize the bundled daemon (config, store, supervisor, logging), build the
/// runtime (in-process backend, or a remote near node when `[link] backend_url`
/// is set), compose the `/v1` API + OAuth routers, optionally serve the
/// runtime↔backend link, and spawn the Axum server. Returns immediately.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-accounts#initialization
pub async fn start_server(server_config: ServerConfig) -> ServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let roots = resolve_roots();

    let settings_repo =
        TomlConfigRepository::open(&roots.config_root).expect("failed to open config directory");
    let daemon = read_daemon_settings(&settings_repo).expect("failed to read runtime settings");
    let config_was_empty = settings_repo.is_empty();
    drop(settings_repo);

    let log_guard = logging::init(&roots.state_root, &daemon.log_level);

    // Runtime↔backend transport: a remote backend when `[link] backend_url` is
    // configured (this process is then a near node over the link), else the
    // in-process default ([replication L1 §10](../replication/L1.md)).
    let backend_transport = match &daemon.link_backend_url {
        Some(base_url) => BackendTransportConfig::Remote {
            base_url: base_url.clone(),
            token: daemon.link_token.clone(),
        },
        None => BackendTransportConfig::InProcess,
    };

    let build_config = RuntimeBuildConfig::new(
        roots.config_root.clone(),
        roots.state_root.clone(),
        roots.state_root.join("cache"),
    )
    .with_bootstrap_path_option(roots.bootstrap_path.clone())
    .with_poll_interval(Duration::from_secs(daemon.poll_interval_seconds))
    .with_backend_transport(backend_transport.clone());

    // A remote backend makes this process a LEAN near node (reads/writes cross
    // the link, the down-channel drives views); otherwise the full bundled graph
    // is built in-process. Only an in-process backend can serve the link or run
    // the OAuth holdout.
    let (runtime_handle, runtime_shutdown, secret_store, link_serve_transport, oauth_mutations) =
        if matches!(backend_transport, BackendTransportConfig::Remote { .. }) {
            let build = build_remote_runtime(build_config).expect("failed to build remote runtime");
            (build.handle, build.shutdown, build.secret_store, None, None)
        } else {
            let build = build_authority_runtime(build_config)
                .await
                .expect("failed to build authority runtime");
            let link_transport = daemon
                .link_serve
                .then(|| build.backend_link.transport().clone());
            (
                build.handle,
                build.shutdown,
                build.secret_store,
                link_transport,
                Some(build.account_mutations),
            )
        };

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

    let state = build_app_state(
        runtime_handle,
        &secret_store,
        &roots,
        &daemon,
        &server_config,
    );

    // The `/v1` router = the near API router merged with the far OAuth router
    // (its own state + the same macaroon perimeter).
    let oauth_state = Arc::new(OAuthState {
        app: state.clone(),
        oauth_flows: Arc::new(OAuthFlowStore::default()),
        oauth_mutations,
    });
    // The bundled server documents + serves its OAuth routes, so it serves the
    // far OpenAPI document (the lean near node serves the OAuth-free near doc).
    let v1_router = build_api_router(state.clone())
        .merge(build_oauth_router(oauth_state))
        .merge(crate::openapi::openapi_router(crate::openapi::document()));

    // Backend role: serve the runtime↔backend link for a remote runtime, with
    // its OWN bearer-token auth. Fail closed under require_auth without a token.
    let mut root_merges = Vec::new();
    if let Some(link_transport) = link_serve_transport {
        let link_auth = if daemon.require_auth {
            match &daemon.link_token {
                Some(token) => LinkAuth::Bearer(token.clone()),
                None => panic!(
                    "[link] serve is enabled under require_auth but no [link] token is set \
                     (set [link].token or POSTHASTE_LINK_TOKEN)"
                ),
            }
        } else {
            LinkAuth::Disabled
        };
        ph_info!(
            events::LINK_SURFACE_SERVED,
            authenticated = daemon.require_auth,
            "serving the runtime↔backend link surface for a remote runtime"
        );
        root_merges.push(link_router(link_transport, link_auth));
    }

    let mut cors_origins = vec![daemon.cors_origin.clone()];
    cors_origins.extend(server_config.extra_cors_origins.iter().cloned());
    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| daemon.bind_address.clone());

    serve(ServeOptions {
        v1_router,
        root_merges,
        bind_address,
        cors_origins,
        frontend_dist: server_config.frontend_dist.clone(),
        auth_token: state.auth_token.clone(),
        require_auth: daemon.require_auth,
        config_root_display: roots.config_root.display().to_string(),
        log_guard,
        runtime_shutdown,
    })
    .await
}
