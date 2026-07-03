//! `posthaste-runtimed`: the lean remote runtime daemon.
//!
//! A runtime near node over a remote authority server (`[link] authority_server_url`) that serves
//! the `/v1` client platform to local clients — drafts, smart mailboxes, and
//! view state shared via the link model. It links only the near crates
//! (`posthaste-http-api-adapter` + `posthaste-runtime`), never store/engine/imap. OAuth
//! provider-account setup is an authority server operation and is not served here.
//!
//! Config is the usual `app.toml` + `POSTHASTE_*` env; the authority server link is
//! `[link] authority_server_url` (+ `[link] token`). Bind with `POSTHASTE_BIND`.

use posthaste_http_api_adapter::{
    assemble_daemon_preamble, build_api_router, build_app_state, serve, DaemonPreamble,
    ServeOptions, ServerConfig,
};
use posthaste_runtime::{build_remote_runtime, AuthorityServerTransportConfig};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let DaemonPreamble {
        roots,
        daemon,
        log_guard,
        build_config,
        ..
    } = assemble_daemon_preamble();

    let base_url = daemon.link_authority_server_url.clone().unwrap_or_else(|| {
        panic!(
            "posthaste-runtimed is a remote near node: set [link] authority_server_url \
             (or POSTHASTE_LINK_AUTHORITY_SERVER_URL) to the authority server it should connect to"
        )
    });

    let build_config = build_config.with_authority_server_transport(AuthorityServerTransportConfig::Remote {
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
        v1_router: build_api_router(state.clone()).merge(posthaste_http_api_adapter::openapi::openapi_router(
            posthaste_http_api_adapter::openapi::document(),
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
        shutdown_token: CancellationToken::new(),
        // A lean near node: no in-process supervisor or store to close (they live
        // in the remote authority server), so those teardown seams are absent.
        supervisor_stop: None,
        store_close: None,
        tls: daemon.tls.clone(),
    })
    .await;

    // Serve until a shutdown signal, then run the ordered teardown (D60/M20).
    handle.into_shutdown_sequence().run_until_signal().await;
}
