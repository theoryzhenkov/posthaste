//! `posthaste-runtimed`: the lean remote runtime daemon.
//!
//! A runtime near node over a remote backend (`[link] backend_url`) that serves
//! the `/v1` client platform to local clients — drafts, smart mailboxes, and
//! view state shared via the session model. It links only the near crates
//! (`posthaste-api` + `posthaste-runtime`), never store/engine/imap. OAuth
//! provider-account setup is a backend operation and is not served here.
//!
//! Config is the usual `app.toml` + `POSTHASTE_*` env; the backend link is
//! `[link] backend_url` (+ `[link] token`). Bind with `POSTHASTE_BIND`.

use std::time::Duration;

use posthaste_api::{
    build_api_router, build_app_state, logging, resolve_roots, serve, ServeOptions, ServerConfig,
};
use posthaste_runtime::{build_remote_runtime, BackendTransportConfig, RuntimeBuildConfig};

#[tokio::main]
async fn main() {
    let roots = resolve_roots();
    let daemon = posthaste_api::load_daemon_settings(&roots.config_root);
    let log_guard = logging::init(&roots.state_root, &daemon.log_level);

    let base_url = daemon.link_backend_url.clone().unwrap_or_else(|| {
        panic!(
            "posthaste-runtimed is a remote near node: set [link] backend_url \
             (or POSTHASTE_LINK_BACKEND_URL) to the backend it should connect to"
        )
    });

    let build_config = RuntimeBuildConfig::new(
        roots.config_root.clone(),
        roots.state_root.clone(),
        roots.state_root.join("cache"),
    )
    .with_poll_interval(Duration::from_secs(daemon.poll_interval_seconds))
    .with_backend_transport(BackendTransportConfig::Remote {
        base_url,
        token: daemon.link_token.clone(),
    });

    let build = build_remote_runtime(build_config).expect("failed to build remote runtime");

    let server_config = ServerConfig::default();
    let state = build_app_state(
        build.handle,
        &build.secret_store,
        &roots,
        &daemon,
        &server_config,
    );

    let mut cors_origins = vec![daemon.cors_origin.clone()];
    cors_origins.extend(server_config.extra_cors_origins.iter().cloned());
    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| daemon.bind_address.clone());

    let handle = serve(ServeOptions {
        // The lean near node serves the OAuth-free near OpenAPI document (it has
        // no provider machinery to run the OAuth flow).
        v1_router: build_api_router(state.clone()).merge(posthaste_api::openapi::openapi_router(
            posthaste_api::openapi::document(),
        )),
        root_merges: Vec::new(),
        bind_address,
        cors_origins,
        frontend_dist: None,
        auth_token: state.auth_token.clone(),
        require_auth: daemon.require_auth,
        config_root_display: roots.config_root.display().to_string(),
        log_guard,
        runtime_shutdown: build.shutdown,
    })
    .await;

    handle
        .join_handle
        .await
        .expect("posthaste-runtimed server task panicked");
}
