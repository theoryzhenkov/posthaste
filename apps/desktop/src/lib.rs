use posthaste_server::ServerConfig;
use serde::Deserialize;
use tauri::webview::WebviewWindow;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager};
use tauri_utils::config::WebviewUrl;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SurfaceDescriptor {
    #[serde(rename = "message")]
    Message {
        disposition: SurfaceDisposition,
        params: MessageSurfaceParams,
    },
    #[serde(rename = "settings")]
    Settings {
        disposition: SurfaceDisposition,
        params: SettingsSurfaceParams,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SurfaceDisposition {
    #[serde(rename = "focused")]
    Focused,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageSurfaceParams {
    conversation_id: String,
    source_id: String,
    message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsSurfaceParams {
    category: Option<String>,
    account_id: Option<String>,
    smart_mailbox_id: Option<String>,
}

/// Receive a log entry from the frontend and emit it through the backend's
/// tracing subscriber so it lands in the same log files with rotation.
#[tauri::command]
fn log_from_frontend(level: &str, domain: &str, message: &str) {
    match level {
        "error" => tracing::error!(target: "frontend", domain, "{message}"),
        "warn" => tracing::warn!(target: "frontend", domain, "{message}"),
        "info" => tracing::info!(target: "frontend", domain, "{message}"),
        "debug" => tracing::debug!(target: "frontend", domain, "{message}"),
        _ => tracing::trace!(target: "frontend", domain, "{message}"),
    }
}

#[tauri::command]
fn open_surface_window(app: AppHandle, surface: SurfaceDescriptor) -> Result<(), String> {
    let label = surface_window_label(&surface);
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let port = app.state::<EmbeddedBackend>().port;
    let route = surface_route(&surface);
    let title = surface_title(&surface);
    let (width, height) = surface_window_size(&surface);
    build_window(
        &app,
        &label,
        &format!("index.html#{route}"),
        title,
        width,
        height,
        port,
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

struct EmbeddedBackend {
    port: u16,
}

/// Run the PostHaste desktop application.
///
/// Starts the embedded Axum backend on an OS-assigned port, then opens a Tauri
/// webview. The port is injected into the webview via `initialization_script`
/// as `window.__POSTHASTE_PORT__` so the frontend can discover the backend.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            log_from_frontend,
            open_surface_window
        ])
        .setup(|app| {
            let config = ServerConfig {
                extra_cors_origins: vec![
                    "https://tauri.localhost".to_string(),
                    "tauri://localhost".to_string(),
                ],
                bind_address_override: Some("127.0.0.1:0".to_string()),
                frontend_dist: None,
            };
            let handle = tauri::async_runtime::block_on(posthaste_server::start_server(config));
            let port = handle.addr.port();
            tracing::info!(addr = %handle.addr, "embedded backend started");
            app.manage(handle);
            app.manage(EmbeddedBackend { port });

            build_window(
                app.handle(),
                "main",
                "index.html",
                "PostHaste",
                1200.0,
                800.0,
                port,
            )?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PostHaste");
}

fn build_window<M: Manager<R>, R: tauri::Runtime>(
    manager: &M,
    label: &str,
    path: &str,
    title: &str,
    width: f64,
    height: f64,
    port: u16,
) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(manager, label, WebviewUrl::App(path.into()))
        .initialization_script(backend_init_script(port))
        .title(title)
        .inner_size(width, height)
        .resizable(true)
        .build()
}

fn backend_init_script(port: u16) -> String {
    format!(
        "Object.defineProperty(window, '__POSTHASTE_PORT__', {{ value: {port}, writable: false }});"
    )
}

fn surface_route(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Message {
            disposition,
            params,
        } => {
            let _ = disposition;
            format!(
                "/surface/message?conversationId={}&sourceId={}&messageId={}",
                encode_component(&params.conversation_id),
                encode_component(&params.source_id),
                encode_component(&params.message_id)
            )
        }
        SurfaceDescriptor::Settings {
            disposition,
            params,
        } => {
            let _ = disposition;
            let mut pairs = Vec::new();
            push_query_pair(&mut pairs, "category", params.category.as_deref());
            push_query_pair(&mut pairs, "accountId", params.account_id.as_deref());
            push_query_pair(
                &mut pairs,
                "smartMailboxId",
                params.smart_mailbox_id.as_deref(),
            );
            if pairs.is_empty() {
                "/surface/settings".to_string()
            } else {
                format!("/surface/settings?{}", pairs.join("&"))
            }
        }
    }
}

fn surface_window_label(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Settings { .. } => "settings".to_string(),
        SurfaceDescriptor::Message { params, .. } => {
            let key = format!("{}:{}", params.source_id, params.message_id);
            format!("message-{:016x}", stable_hash(key.as_bytes()))
        }
    }
}

fn surface_title(surface: &SurfaceDescriptor) -> &'static str {
    match surface {
        SurfaceDescriptor::Settings { .. } => "PostHaste Settings",
        SurfaceDescriptor::Message { .. } => "PostHaste Message",
    }
}

fn surface_window_size(surface: &SurfaceDescriptor) -> (f64, f64) {
    match surface {
        SurfaceDescriptor::Settings { .. } => (980.0, 720.0),
        SurfaceDescriptor::Message { .. } => (900.0, 760.0),
    }
}

fn push_query_pair(pairs: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        pairs.push(format!("{key}={}", encode_component(value)));
    }
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_surface_route_uses_hash_route_and_encoded_params() {
        let surface = SurfaceDescriptor::Message {
            disposition: SurfaceDisposition::Focused,
            params: MessageSurfaceParams {
                conversation_id: "conversation/1".to_string(),
                source_id: "source:primary".to_string(),
                message_id: "message 1".to_string(),
            },
        };

        assert_eq!(
            surface_route(&surface),
            "/surface/message?conversationId=conversation%2F1&sourceId=source%3Aprimary&messageId=message%201"
        );
    }

    #[test]
    fn message_window_label_is_stable_and_safe() {
        let surface = SurfaceDescriptor::Message {
            disposition: SurfaceDisposition::Focused,
            params: MessageSurfaceParams {
                conversation_id: "conversation/1".to_string(),
                source_id: "source:primary".to_string(),
                message_id: "message 1".to_string(),
            },
        };

        assert!(surface_window_label(&surface).starts_with("message-"));
        assert_eq!(
            surface_window_label(&surface),
            surface_window_label(&surface)
        );
    }

    #[test]
    fn settings_window_reuses_stable_label() {
        let surface = SurfaceDescriptor::Settings {
            disposition: SurfaceDisposition::Focused,
            params: SettingsSurfaceParams {
                category: Some("accounts".to_string()),
                account_id: Some("primary".to_string()),
                smart_mailbox_id: None,
            },
        };

        assert_eq!(surface_window_label(&surface), "settings");
        assert_eq!(
            surface_route(&surface),
            "/surface/settings?category=accounts&accountId=primary"
        );
    }
}
