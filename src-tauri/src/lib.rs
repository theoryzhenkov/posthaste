use tauri::Manager;
use web_server::ServerConfig;

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
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let handle = web_server::start_server(config).await;
                tracing::info!(addr = %handle.addr, "embedded backend started");
                // Store in managed state so the log guard and server task survive
                // for the application lifetime. Not accessed by Tauri commands.
                app_handle.manage(handle);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PostHaste");
}
