use std::net::SocketAddr;
use std::time::Duration;

use super::*;
use posthaste_link_contract::RuntimeId;

/// Handle returned by [`start_backend`]: the bound address, the server task, and
/// the log guard (must outlive the process).
pub struct BackendServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
}

/// Start the standalone `posthaste-backend` role: build the backend far node and
/// serve ONLY the authenticated runtime↔backend link ([replication backend-link L2 §7](../replication/backend-link/L2.md),
/// assertion `backend-builds-standalone`). No `/v1` client API and no renderer —
/// a remote `posthaste-runtime` drives this backend over the link.
///
/// @spec docs/replication/backend-link/L2#7-the-build-seam-and-role-binaries
pub async fn start_backend(server_config: ServerConfig) -> BackendServerHandle {
    #[cfg(debug_assertions)]
    dotenv().ok();

    let roots = resolve_roots();

    let settings_repo =
        TomlConfigRepository::open(&roots.config_root).expect("failed to open config directory");
    let runtime =
        config::read_daemon_settings(&settings_repo).expect("failed to read runtime settings");
    drop(settings_repo);

    let log_guard = logging::init(&roots.state_root, &runtime.log_level);

    let node = build_backend_node(
        RuntimeBuildConfig::new(
            roots.config_root.clone(),
            roots.state_root.clone(),
            roots.state_root.join("cache"),
        )
        .with_bootstrap_path_option(roots.bootstrap_path.clone())
        .with_poll_interval(Duration::from_secs(runtime.poll_interval_seconds)),
    )
    .await
    .expect("failed to build backend node");

    // The link is the ONLY surface here; require [link].runtimes under
    // require_auth and fail closed if absent (the backend is entirely
    // network-exposed). With require_auth off (explicit dev opt-out) it serves
    // unauthenticated.
    let link_auth = if runtime.require_auth {
        match &runtime.link_runtimes {
            Some(map) if !map.is_empty() => LinkAuth::PerRuntime(
                map.iter()
                    .map(|(token, rid)| (token.clone(), RuntimeId::new(rid.clone())))
                    .collect(),
            ),
            _ => panic!(
                "posthaste-backend requires [link].runtimes (token → runtime_id) under \
                 require_auth — one entry per connecting runtime (X ≥ 1)"
            ),
        }
    } else {
        LinkAuth::Disabled
    };

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            // SECURITY: log only the path — never the query string or the
            // `Authorization` header (the link bearer token).
            info_span!(
                "http.request",
                method = %request.method(),
                path = %request.uri().path(),
                process_id = std::process::id(),
                process_role = "backend",
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

    let bind_address = server_config
        .bind_address_override
        .clone()
        .unwrap_or_else(|| runtime.bind_address.clone());
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind backend listener");
    let addr = listener.local_addr().expect("failed to get local address");

    ph_info!(
        events::LINK_SURFACE_SERVED,
        address = %addr,
        authenticated = runtime.require_auth,
        account_count = node.runtime_status().account_count,
        "posthaste-backend serving the runtime↔backend link surface"
    );

    let join_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("posthaste-backend server failed");
    });

    BackendServerHandle {
        addr,
        join_handle,
        log_guard,
    }
}
