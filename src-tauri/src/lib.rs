use tauri::webview::WebviewWindowBuilder;
use tauri_utils::config::WebviewUrl;
use tauri::{Manager, State};
use web_server::{ServerConfig, ServerHandle};

#[tauri::command]
fn get_api_port(handle: State<'_, ServerHandle>) -> u16 {
    handle.addr.port()
}

/// Run the PostHaste desktop application.
///
/// Starts the embedded Axum backend on an OS-assigned port, then opens a Tauri
/// webview that discovers the port via the `get_api_port` command. The CSP is
/// set dynamically to allow connections to the actual bound port.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_api_port])
        .setup(|app| {
            let config = ServerConfig {
                extra_cors_origins: vec!["https://tauri.localhost".to_string()],
                bind_address_override: Some("127.0.0.1:0".to_string()),
            };
            let handle = tauri::async_runtime::block_on(web_server::start_server(config));
            let port = handle.addr.port();
            tracing::info!(addr = %handle.addr, "embedded backend started");
            app.manage(handle);

            let csp = format!(
                "default-src 'self'; \
                 connect-src 'self' http://127.0.0.1:{port} ws://127.0.0.1:{port}; \
                 style-src 'self' 'unsafe-inline'; \
                 font-src 'self' data:; \
                 img-src 'self' data: blob:"
            );

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("PostHaste")
                .inner_size(1200.0, 800.0)
                .resizable(true)
                .on_web_resource_request(move |_req, response| {
                    response.headers_mut().insert(
                        "Content-Security-Policy",
                        csp.parse().unwrap(),
                    );
                })
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PostHaste");
}
