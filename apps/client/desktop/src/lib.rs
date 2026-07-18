//! The integrated Posthaste desktop app: a Tauri shell around the embedded
//! client backend. A second launch hands off to the running instance via the
//! single-instance plugin; startup order is paths → factory-reset marker →
//! logging → backend (assemble + serve + connection info) → menu → main
//! window; exit reverses it (remove connection info, stop runtimes, close
//! the store).

use posthaste_client_backend::AppPaths;
use posthaste_observability::{events, ph_error, ph_info};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use app_menu::build_app_menu;
use injection::{BackendInjection, FocusedWindowLabel};
use windows::{
    build_window, close_remembered_webview_window, open_external_url, open_surface_window,
    surface_webview_booted, toggle_devtools, WebviewBootAcks,
};

mod app_menu;
mod backend;
mod frontend_logging;
mod injection;
mod logging;
mod maintenance;
mod surfaces;
mod windows;

const CLOSE_WINDOW_MENU_ID: &str = "close-window";
const CLOSE_WINDOW_REQUESTED_EVENT: &str = "posthaste://close-window-requested";
const MAIN_WINDOW_LABEL: &str = "main";

/// Compile-time release channel ("nightly" | "stable" | "dev"). Resolved by
/// `build.rs` from the `POSTHASTE_RELEASE_CHANNEL` env var and re-exported as
/// `POSTHASTE_RELEASE_CHANNEL_RESOLVED`; defaults to "dev" for local builds.
/// Going through the build script (rather than `option_env!`) makes the channel
/// a first-class cargo build input with proper fingerprinting, so a stale
/// `target/`/sccache object can never carry the wrong channel.
///
/// Drives the updater-endpoint binding and the `--print-release-channel`
/// self-report the release smoke checks run.
pub const RELEASE_CHANNEL: &str = env!("POSTHASTE_RELEASE_CHANNEL_RESOLVED");

/// Flag that makes the binary print its compiled-in [`RELEASE_CHANNEL`] and
/// exit, before any GUI initialization. The release smoke step runs this and
/// compares the output to the expected channel — a direct, toolchain-independent
/// proof that an artifact was built for the right channel.
pub const PRINT_RELEASE_CHANNEL_FLAG: &str = "--print-release-channel";

/// If invoked with [`PRINT_RELEASE_CHANNEL_FLAG`], print the channel and return
/// `true` so the caller can exit before starting Tauri. Kept out of `run()` so
/// it works headless (no event loop, no display).
pub fn handle_print_release_channel() -> bool {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == PRINT_RELEASE_CHANNEL_FLAG)
    {
        println!("{RELEASE_CHANNEL}");
        true
    } else {
        false
    }
}

/// Holds the log-flush guard for the process lifetime.
struct LogFlushGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

/// Run the integrated Posthaste desktop application.
///
/// Starts the embedded backend on an OS-assigned loopback port with a
/// per-launch session token, then opens the Tauri webview. Port and token are
/// injected into every webview via `initialization_script` as
/// `window.__POSTHASTE_PORT__` / `window.__POSTHASTE_TOKEN__` so the frontend
/// discovers the backend before the page loads.
pub fn run() {
    let builder = tauri::Builder::default()
        // One app instance per user session: a second launch hands off to
        // the running instance (which refocuses its main window) and exits
        // before any plugin or the backend initializes — so it can never
        // open the store a second time or clobber the connection-info file.
        // Registered first so the handoff runs before everything else. The
        // guard is keyed to the per-channel bundle identifier; the
        // cross-channel guard over the shared store is the state-root lock
        // inside `AppState::assemble`.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Native error dialogs (backend startup failure).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Auto-update support: the updater checks the GitHub Releases manifest
        // and the process plugin relaunches after an update is installed. Both
        // are inert until the frontend invokes a check.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // New-mail OS banners. Delivery is driven entirely from the renderer:
        // the arrival gate decides, the plugin's JS API posts. Permission is
        // requested lazily on first enable — never at boot.
        .plugin(tauri_plugin_notification::init());

    let builder = builder.on_menu_event(|app, event| {
        if event.id().as_ref() == CLOSE_WINDOW_MENU_ID {
            close_remembered_webview_window(app);
        }
    });

    let builder = builder.invoke_handler(tauri::generate_handler![
        frontend_logging::log_from_frontend,
        open_external_url,
        open_surface_window,
        surface_webview_booted,
        toggle_devtools,
        maintenance::request_database_repair,
        maintenance::request_factory_reset,
        maintenance::get_diagnostics_info,
        maintenance::reveal_log_folder
    ]);

    let builder = builder.setup(|app| {
        let paths = AppPaths::resolve();

        // A requested factory reset must run before logging opens a file in
        // the state root and before the embedded backend opens any store
        // file. Best-effort; the renderer already cleared its UI state and
        // will confirm on this boot.
        let factory_reset_performed = maintenance::consume_factory_reset_marker(&paths);

        app.manage(LogFlushGuard(logging::init(&paths.state_root)));

        // Log the baked release channel so it is observable in diagnostics.
        ph_info!(
            events::DESKTOP_RELEASE_CHANNEL,
            channel = RELEASE_CHANNEL,
            "desktop release channel"
        );
        if factory_reset_performed {
            ph_info!(events::DESKTOP_FACTORY_RESET, "factory reset performed");
        }

        let embedded = match tauri::async_runtime::block_on(backend::start(paths)) {
            Ok(embedded) => embedded,
            Err(message) => {
                // Reached when the backend cannot start — including the
                // state-root lock being held by another instance the
                // single-instance guard cannot see (another channel's build
                // or the standalone backend binary over the shared store).
                // Surface the error and exit; the dialog callback drives the
                // exit because the event loop is not running yet, so a
                // blocking dialog here would never be pumped.
                ph_error!(
                    events::DESKTOP_BACKEND_START_FAILED,
                    error = %message,
                    "embedded backend failed to start"
                );
                let exit_handle = app.handle().clone();
                app.dialog()
                    .message(&message)
                    .kind(MessageDialogKind::Error)
                    .title("Posthaste")
                    .show(move |_| exit_handle.exit(1));
                return Ok(());
            }
        };
        ph_info!(
            events::DESKTOP_BACKEND_STARTED,
            port = embedded.port,
            "embedded backend started"
        );
        let injection = BackendInjection {
            port: embedded.port,
            auth_token: embedded.auth_token.clone(),
        };
        app.manage(embedded);

        app.manage(FocusedWindowLabel::new(MAIN_WINDOW_LABEL));
        app.manage(WebviewBootAcks::default());

        app.set_menu(build_app_menu(app)?)?;

        build_window(
            app.handle(),
            MAIN_WINDOW_LABEL,
            "index.html",
            "Posthaste",
            1200.0,
            800.0,
            &injection,
        )?;

        Ok(())
    });

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building Posthaste");
    // Ordered teardown on exit: remove the connection-info file so a closed
    // app leaves no stale port/credential behind, then stop the account
    // runtimes and close the store.
    app.run(move |app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(embedded) = app_handle.try_state::<backend::EmbeddedBackend>() {
                tauri::async_runtime::block_on(backend::stop(&embedded));
            }
        }
    });
}
