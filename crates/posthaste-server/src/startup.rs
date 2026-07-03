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
    // The shared shutdown token, created up front so the in-process rule engine
    // can tie its lifetime to it (dropped ⇒ engine stops) alongside axum's drain.
    let shutdown_token = CancellationToken::new();

    #[allow(clippy::type_complexity)]
    let (
        runtime_handle,
        runtime_shutdown,
        secret_store,
        link_serve_transport,
        oauth_mutations,
        supervisor_stop,
        store_close,
        rules_managed,
    ) = if matches!(authority_server_transport, AuthorityServerTransportConfig::Remote { .. }) {
        // A lean near node: no in-process supervisor or store to tear down (they
        // live in the remote authority server), so those teardown seams are absent.
        // No local authority server ⇒ no rule engine and no rule write surface.
        let build = build_remote_runtime(build_config).expect("failed to build remote runtime");
        (
            build.handle,
            build.shutdown,
            build.secret_store,
            None,
            None,
            None,
            None,
            None,
        )
    } else {
        let build = build_authority_server(build_config)
            .await
            .expect("failed to build authority runtime");
        let link_transport = daemon
            .link_serve
            .then(|| build.authority_server_link.clone());
        // The bundled server owns the supervisor + store, so it wires both
        // teardown seams into the shutdown sequence (D60 steps (b) and (c)).
        let supervisor_stop: Option<Box<dyn SupervisorStop>> = Some(Box::new(
            AccountSupervisorStop(build.account_supervisor.clone()),
        ));
        let store_close: Option<Box<dyn StoreClose>> =
            Some(Box::new(DatabaseStoreClose(build.database_store.clone())));

        // Spawn the in-process automation rule engine over the far node's event
        // bus (RFC-L2-scripting S5). The minter signs per-invocation hook tokens
        // with the SAME macaroon root key the `/v1` perimeter verifies (resolved
        // deterministically from the same secret store + state root that
        // `build_app_state` uses below, so the keys match). Only spawned when
        // `rules.toml` has enabled rules; the handle is dropped at shutdown.
        let macaroon_root_key = token::resolve_root_key(build.secret_store.as_ref(), &roots.state_root);
        let minter = Arc::new(MacaroonMinter::new(macaroon_root_key));
        // Always spawns (even with zero rules) so the REST write surface's
        // ManagedRulesHandle can hot-reload a GUI-created rule into the live
        // evaluator without a restart (RFC-L2-scripting ruling 23).
        let engine = build.spawn_rule_engine(Some(minter));
        let rules_managed = Some(engine.managed_rules());
        let engine_stop = shutdown_token.clone();
        tokio::spawn(async move {
            engine_stop.cancelled().await;
            drop(engine);
        });

        (
            build.handle,
            build.shutdown,
            build.secret_store,
            link_transport,
            Some(build.account_mutations),
            supervisor_stop,
            store_close,
            rules_managed,
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
    let oauth_flows = Arc::new(OAuthFlowStore::default());
    // M27(a): the periodic pending-flow sweep (defense-in-depth beside
    // prune-on-insert); stops with the shutdown token.
    let flow_sweep_cancel = CancellationToken::new();
    oauth_flows.clone().spawn_sweep_task(flow_sweep_cancel.clone());
    let oauth_state = Arc::new(OAuthState {
        app: state.clone(),
        oauth_flows,
        oauth_mutations,
    });
    // The bundled server documents + serves its OAuth routes, so it serves the
    // far OpenAPI document (the lean near node serves the OAuth-free near doc).
    let v1_router = build_api_router(state.clone())
        .merge(build_oauth_router(oauth_state))
        // The automation-rules surface (bundled-only): read-only list of the
        // merged ruleset, plus the safe write routes (create/update/delete) when
        // a local rule engine exists (RFC-L2-scripting ruling 23).
        .merge(crate::rules_api::build_rules_router(state.clone(), rules_managed))
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
        shutdown_token: {
            let sweep = flow_sweep_cancel;
            let child = shutdown_token.clone();
            tokio::spawn(async move {
                child.cancelled().await;
                sweep.cancel();
            });
            shutdown_token
        },
        supervisor_stop,
        store_close,
        tls: daemon.tls.clone(),
    })
    .await
}
