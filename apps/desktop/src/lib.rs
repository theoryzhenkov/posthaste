use posthaste_observability::{
    events, ph_forwarded_debug, ph_forwarded_error, ph_forwarded_info, ph_forwarded_trace,
    ph_forwarded_warn, ph_info,
};
#[cfg(feature = "embedded-server")]
use posthaste_server::ServerConfig;
use serde::Deserialize;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuBuilder, SubmenuBuilder};
use tauri::webview::WebviewWindow;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use tauri_utils::config::WebviewUrl;

use app_menu::build_app_menu;
use backend_injection::{
    backend_init_script, BackendInjection, EmbeddedBackend, FocusedWindowLabel,
};
use desktop_windows::{
    build_window, close_remembered_webview_window, open_external_url, open_surface_window,
    toggle_devtools,
};
use frontend_logging::log_from_frontend;
#[cfg(test)]
use frontend_logging::log_token;
use surface_routes::{
    surface_route, surface_title, surface_window_label, surface_window_navigation_script,
    surface_window_size, validate_surface_descriptor,
};
use surface_types::*;

mod app_menu;
mod backend_injection;
mod client_connection;
mod desktop_windows;
mod frontend_logging;
mod surface_routes;
mod surface_types;

#[cfg(feature = "e2e-testing")]
mod e2e;

const CLOSE_WINDOW_MENU_ID: &str = "close-window";
const CLOSE_WINDOW_REQUESTED_EVENT: &str = "posthaste://close-window-requested";
const MAIN_WINDOW_LABEL: &str = "main";

/// Compile-time release channel ("nightly" | "stable" | "dev"). Set via the
/// `POSTHASTE_RELEASE_CHANNEL` env var at build time; defaults to "dev" for
/// local builds. Drives in-product channel display and the updater-endpoint
/// binding (see docs/eph/DESIGN-L2-release-channels.md).
pub(crate) const RELEASE_CHANNEL: &str = match option_env!("POSTHASTE_RELEASE_CHANNEL") {
    Some(channel) => channel,
    None => "dev",
};

/// Sentinel baked into the binary so the release smoke step can prove which
/// channel a binary was built on by grepping for `posthaste-release-channel=`.
/// The CI sets `POSTHASTE_RELEASE_CHANNEL_SENTINEL` to the full sentinel; local
/// builds default to the dev sentinel. `concat!` cannot take a const, so the
/// full string is supplied by the build environment rather than built here.
#[used]
static RELEASE_CHANNEL_SENTINEL: &str = match option_env!("POSTHASTE_RELEASE_CHANNEL_SENTINEL") {
    Some(sentinel) => sentinel,
    None => "posthaste-release-channel=dev",
};

/// Return the compile-time release channel to the renderer so the desktop binary
/// and the web bundle cannot silently disagree.
#[tauri::command]
fn release_channel() -> &'static str {
    RELEASE_CHANNEL
}

#[cfg(all(feature = "e2e-testing", not(target_os = "linux")))]
compile_error!("PostHaste e2e-testing is Linux-only; macOS release smoke remains manual");

/// Run the Posthaste desktop application.
///
/// Starts the embedded Axum backend on an OS-assigned port, then opens a Tauri
/// webview. The port is injected into the webview via `initialization_script`
/// as `window.__POSTHASTE_PORT__` so the frontend can discover the backend.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Auto-update support: the updater checks the GitHub Releases manifest
        // and the process plugin relaunches after an update is installed. Both
        // are inert until the frontend invokes a check (see useDesktopUpdates).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    let builder = builder.on_menu_event(|app, event| {
        if event.id().as_ref() == CLOSE_WINDOW_MENU_ID {
            close_remembered_webview_window(app);
        }
    });

    #[cfg(feature = "e2e-testing")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        log_from_frontend,
        open_external_url,
        open_surface_window,
        toggle_devtools,
        release_channel,
        client_connection::client_connections_read,
        client_connection::client_connections_write,
        client_connection::client_token_get,
        client_connection::client_token_set,
        client_connection::client_token_delete,
        client_connection::client_local_daemon_read,
        client_connection::request_database_repair,
        e2e::posthaste_e2e_result
    ]);
    #[cfg(not(feature = "e2e-testing"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        log_from_frontend,
        open_external_url,
        open_surface_window,
        toggle_devtools,
        release_channel,
        client_connection::client_connections_read,
        client_connection::client_connections_write,
        client_connection::client_token_get,
        client_connection::client_token_set,
        client_connection::client_token_delete,
        client_connection::client_local_daemon_read,
        client_connection::request_database_repair
    ]);

    let builder = builder.setup(|app| {
        // Log the baked release channel so the sentinel is reachable (survives
        // linker GC) and so the channel is observable in diagnostics.
        ph_info!(
            events::DESKTOP_RELEASE_CHANNEL,
            channel = RELEASE_CHANNEL,
            "desktop release channel"
        );
        let _ = RELEASE_CHANNEL_SENTINEL;

        #[cfg(feature = "embedded-server")]
        let backend = {
            let config = ServerConfig {
                extra_cors_origins: vec![
                    "https://tauri.localhost".to_string(),
                    "tauri://localhost".to_string(),
                    "http://127.0.0.1:5173".to_string(),
                ],
                bind_address_override: Some("127.0.0.1:0".to_string()),
                frontend_dist: None,
            };
            let handle = tauri::async_runtime::block_on(posthaste_server::start_server(config));
            let port = handle.addr.port();
            let auth_token = handle.auth_token.clone();
            ph_info!(
                events::DESKTOP_BACKEND_STARTED,
                addr = %handle.addr,
                "embedded backend started"
            );
            app.manage(handle);
            app.manage(EmbeddedBackend {
                port,
                auth_token: auth_token.clone(),
            });
            BackendInjection { port, auth_token }
        };
        // Client-only build: no embedded server, so nothing is injected. The
        // connection-profile runtime that supplies a backend in this mode lands
        // in Phase B; the frontend's `desktop.ts` guards degrade gracefully when
        // `__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__` are absent.
        #[cfg(not(feature = "embedded-server"))]
        let backend = BackendInjection::none();

        app.manage(FocusedWindowLabel::new(MAIN_WINDOW_LABEL));
        #[cfg(feature = "e2e-testing")]
        app.manage(e2e::E2eBridgeState::default());

        app.set_menu(build_app_menu(app)?)?;

        build_window(
            app.handle(),
            MAIN_WINDOW_LABEL,
            "index.html",
            "Posthaste",
            1200.0,
            800.0,
            &backend,
        )?;

        #[cfg(feature = "e2e-testing")]
        e2e::start_e2e_bridge(app.handle().clone());

        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running Posthaste");
}

#[cfg(test)]
use desktop_windows::{
    is_closeable_surface_window_label, is_external_web_url, is_main_window_label,
    validate_external_url,
};

#[cfg(test)]
mod tests;
