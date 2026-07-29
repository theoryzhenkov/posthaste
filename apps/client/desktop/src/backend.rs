//! The embedded backend: assemble [`AppState`] over the default [`AppPaths`],
//! serve the loopback API on an ephemeral port with a per-launch session
//! token, and publish the connection info.
//!
//! Discovery is ONE mechanism: the `connection-info.json` document the
//! standalone backend binary also writes. The shell writes it after the bind
//! and removes it on exit, so local consumers (CLI, scripts) find the same
//! port + token the webviews get injected.

use std::path::PathBuf;

use axum::http::{HeaderValue, Method};
use posthaste_client_backend::{AppPaths, AppState, BuildOptions, ConnectionInfo, ServerHandle};
use tower_http::cors::CorsLayer;

/// Webview and dev origins allowed to call the loopback API. The webview
/// serves from the tauri scheme (or `http://tauri.localhost` on Windows
/// WebView2), so every API request is cross-origin and needs CORS; the vite
/// origins cover `tauri dev` against the dev server.
const ALLOWED_ORIGINS: [&str; 5] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://127.0.0.1:5173",
    "http://localhost:5173",
];

/// The running embedded backend: everything the shell needs to inject
/// connection info into webviews and to tear the backend down on exit.
pub(crate) struct EmbeddedBackend {
    pub(crate) state: AppState,
    pub(crate) port: u16,
    pub(crate) auth_token: String,
    pub(crate) info_path: PathBuf,
    /// The serving task, so teardown can stop answering requests *before*
    /// [`AppState::shutdown`] closes the store the handlers read through.
    server: ServerHandle,
}

/// Assemble the backend service core over `paths`, bind the API on an
/// ephemeral loopback port with a fresh session token, and write the
/// connection-info file. Must run within a tokio runtime.
pub(crate) async fn start(paths: AppPaths) -> Result<EmbeddedBackend, String> {
    let state = AppState::assemble(BuildOptions::at(paths))
        .await
        .map_err(|error| format!("failed to start the embedded backend: {error}"))?;

    // The token is minted before the bind so the router enforces it from the
    // first request; the real port lands on the document after.
    let mut info = ConnectionInfo::generate(0);
    let router = posthaste_client_backend::router(state.clone(), info.token.clone()).layer(cors());

    let server = posthaste_client_backend::serve_router(router, 0)
        .await
        .map_err(|error| format!("failed to bind the API port: {error}"))?;

    info.port = server.addr.port();
    let info_path = state.paths.connection_info_path();
    if let Err(error) = info.write(&info_path) {
        server.abort();
        state.shutdown().await;
        return Err(format!(
            "failed to write connection info at {}: {error}",
            info_path.display()
        ));
    }

    Ok(EmbeddedBackend {
        port: server.addr.port(),
        state,
        auth_token: info.token,
        info_path,
        server,
    })
}

/// Ordered teardown: drop the discovery document, stop serving, then run the
/// backend's own teardown. Idempotent; called from the `RunEvent::Exit` hook.
///
/// The server is stopped *before* [`AppState::shutdown`], not after: shutdown
/// stops the account runtimes, closes the store and releases the state-root
/// lock, and any request still in flight — including the SSE stream the
/// webviews hold open — would otherwise be reading a closed store through a
/// handler that has no idea teardown has begun.
pub(crate) async fn stop(backend: &EmbeddedBackend) {
    if let Err(error) = ConnectionInfo::remove(&backend.info_path) {
        tracing::warn!(%error, "failed to remove connection info during shutdown");
    }
    backend.server.abort();
    backend.state.shutdown().await;
}

/// CORS for the webview origins. Preflights are answered by the layer itself,
/// so they never reach the token check (which they could not pass — a
/// preflight carries no Authorization header).
fn cors() -> CorsLayer {
    let origins: Vec<HeaderValue> = ALLOWED_ORIGINS
        .iter()
        .map(|origin| HeaderValue::from_static(origin))
        .collect();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
        ])
}
