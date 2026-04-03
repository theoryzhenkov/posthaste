use tauri::Manager;
use web_server::{ServerConfig, ServerHandle};

/// Run the PostHaste desktop application.
///
/// Starts the embedded Axum backend on localhost, then opens a Tauri webview
/// that connects to it. The backend runs in-process for the lifetime of the app.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let config = ServerConfig {
                extra_cors_origins: vec!["https://tauri.localhost".to_string()],
            };
            let handle: ServerHandle =
                tauri::async_runtime::block_on(web_server::start_server(config));
            tracing::info!(addr = %handle.addr, "embedded backend started");
            app.manage(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PostHaste");
}
