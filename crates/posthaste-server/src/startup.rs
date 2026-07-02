use super::*;

use posthaste_authority_server::oauth::OAuthFlowStore;

/// Initialize the bundled daemon (config, store, supervisor, logging), build the
/// runtime (in-process authority server, or a remote near node when `[link] authority_server_url`
/// is set), compose the `/v1` API + OAuth routers, optionally serve the
/// runtime↔authority-server link, and spawn the Axum server. Returns immediately.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-accounts#initialization
pub async fn start_server(server_config: ServerConfig) -> ServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let DaemonPreamble {
        roots,
        daemon,
        log_guard,
        build_config,
        config_was_empty,
    } = assemble_daemon_preamble();

    // Runtime↔authority server transport: a remote authority server when `[link] authority_server_url` is
    // configured (this process is then a near node over the link), else the
    // in-process default ([replication L1 §10](../replication/L1.md)).
    let authority_server_transport = match &daemon.link_authority_server_url {
        Some(base_url) => AuthorityServerTransportConfig::Remote {
            base_url: base_url.clone(),
            token: daemon.link_token.clone(),
        },
        None => AuthorityServerTransportConfig::InProcess,
    };

    let build_config = build_config
        .with_bootstrap_path_option(roots.bootstrap_path.clone())
        .with_authority_server_transport(authority_server_transport.clone());

    // A remote authority server makes this process a LEAN near node (reads/writes cross
    // the link, the down-channel drives views); otherwise the full bundled graph
    // is built in-process. Only an in-process authority server can serve the link or run
    // the OAuth holdout.
    let (runtime_handle, runtime_shutdown, secret_store, link_serve_transport, oauth_mutations) =
        if matches!(authority_server_transport, AuthorityServerTransportConfig::Remote { .. }) {
            let build = build_remote_runtime(build_config).expect("failed to build remote runtime");
            (build.handle, build.shutdown, build.secret_store, None, None)
        } else {
            let build = build_authority_server(build_config)
                .await
                .expect("failed to build authority runtime");
            let link_transport = daemon
                .link_serve
                .then(|| build.authority_server_link.transport().clone());
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

    // Authority server role: serve the runtime↔authority-server link for a remote runtime, with
    // its OWN per-runtime token auth. Fail closed under require_auth without
    // [link].runtimes.
    let mut root_merges = Vec::new();
    if let Some(link_transport) = link_serve_transport {
        let link_auth = LinkAuth::from_daemon_settings(&daemon, "[link] serve");
        ph_info!(
            events::LINK_SURFACE_SERVED,
            authenticated = daemon.require_auth,
            "serving the runtime↔authority-server link surface for a remote runtime"
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
        tls: daemon.tls.clone(),
    })
    .await
}
