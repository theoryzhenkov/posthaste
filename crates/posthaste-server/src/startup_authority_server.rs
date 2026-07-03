use std::net::SocketAddr;
use std::time::Duration;

use super::*;

/// Handle returned by [`start_authority_server`]: the bound address, the server task, and
/// the log guard (must outlive the process). Also carries the teardown pieces so
/// the role binary can run the ordered [`ShutdownSequence`] (D60/M20): the shared
/// cancellation token (wired into axum's graceful shutdown) and the supervisor +
/// store seams. There is no runtime near node here, so no `RuntimeShutdownHandle`.
pub struct AuthorityServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
    shutdown_token: CancellationToken,
    supervisor_stop: Box<dyn SupervisorStop>,
    store_close: Box<dyn StoreClose>,
}

impl AuthorityServerHandle {
    /// Consume the handle into the ordered [`ShutdownSequence`] (D60). The role
    /// binary reads `addr` up front, then `.run_until_signal()`.
    pub fn into_shutdown_sequence(self) -> ShutdownSequence {
        ShutdownSequence::new(self.shutdown_token, self.join_handle)
            .with_supervisor_stop(self.supervisor_stop)
            .with_store_close(self.store_close)
            .with_log_guard(self.log_guard)
    }
}

/// Start the standalone `posthaste-authority-server` role: build the authority server far node and
/// serve ONLY the authenticated runtime↔authority-server link ([replication authority-server-link L2 §7](../replication/authority-server-link/L2.md),
/// assertion `authority-server-builds-standalone`). No `/v1` client API and no renderer —
/// a remote `posthaste-runtime` drives this authority server over the link.
///
/// @spec docs/replication/authority-server-link/L2#7-the-build-seam-and-role-binaries
pub async fn start_authority_server(server_config: ServerConfig) -> AuthorityServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let DaemonPreamble {
        roots,
        daemon: runtime,
        log_guard,
        build_config,
        ..
    } = assemble_daemon_preamble();

    let node = build_authority_server_node(
        build_config.with_bootstrap_path_option(roots.bootstrap_path.clone()),
    )
    .await
    .expect("failed to build authority server node");

    // The link is the ONLY surface here; fail closed under require_auth without
    // [link].runtimes (the authority server is entirely network-exposed). With
    // require_auth off (explicit dev opt-out) it serves unauthenticated.
    let link_auth = LinkAuth::from_daemon_settings(&runtime, "posthaste-authority-server");

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            // SECURITY: log only the path — never the query string or the
            // `Authorization` header (the link bearer token).
            info_span!(
                "http.request",
                method = %request.method(),
                path = %request.uri().path(),
                process_id = std::process::id(),
                process_role = "authority-server",
                status = field::Empty,
                latency_ms = field::Empty,
            )
        })
        .on_response(
            |response: &axum::http::Response<_>, latency: Duration, span: &Span| {
                span.record("status", response.status().as_u16());
                span.record("latency_ms", latency.as_millis() as u64);
            },
        );

    let app = link_router(node.transport(), link_auth).layer(trace_layer);

    // Teardown seams (D60 steps (b)/(c)): the standalone authority server owns the
    // supervisor + store, so it wires both into its shutdown sequence.
    let supervisor_stop: Box<dyn SupervisorStop> =
        Box::new(AccountSupervisorStop(node.account_supervisor()));
    let store_close: Box<dyn StoreClose> = Box::new(DatabaseStoreClose(node.database_store()));

    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| runtime.bind_address.clone());
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind authority server listener");
    let addr = listener.local_addr().expect("failed to get local address");

    ph_info!(
        events::LINK_SURFACE_SERVED,
        address = %addr,
        authenticated = runtime.require_auth,
        account_count = node.runtime_status().account_count,
        "posthaste-authority-server serving the runtime↔authority-server link surface"
    );

    // Graceful shutdown (D60 phase (a)): cancelling the token drains in-flight
    // link requests before the serve future returns.
    let shutdown_token = CancellationToken::new();
    let drain_token = shutdown_token.clone();
    let join_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(drain_token.cancelled_owned())
            .await
            .expect("posthaste-authority-server server failed");
    });

    AuthorityServerHandle {
        addr,
        join_handle,
        log_guard,
        shutdown_token,
        supervisor_stop,
        store_close,
    }
}
