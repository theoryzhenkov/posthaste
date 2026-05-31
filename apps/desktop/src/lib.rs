#[cfg(feature = "embedded-server")]
use posthaste_observability::{events, ph_info};
use posthaste_observability::{
    ph_forwarded_debug, ph_forwarded_error, ph_forwarded_info, ph_forwarded_trace,
    ph_forwarded_warn,
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

mod client_connection;

#[cfg(feature = "e2e-testing")]
mod e2e;

const CLOSE_WINDOW_MENU_ID: &str = "close-window";
const CLOSE_WINDOW_REQUESTED_EVENT: &str = "posthaste://close-window-requested";
const MAIN_WINDOW_LABEL: &str = "main";

#[cfg(all(feature = "e2e-testing", not(target_os = "linux")))]
compile_error!("PostHaste e2e-testing is Linux-only; macOS release smoke remains manual");

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum SurfaceDescriptor {
    #[serde(rename = "attachment")]
    Attachment {
        disposition: SurfaceDisposition,
        params: AttachmentSurfaceParams,
    },
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
    #[serde(rename = "compose")]
    Compose {
        disposition: SurfaceDisposition,
        params: ComposeSurfaceParams,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SurfaceDisposition {
    #[serde(rename = "focused")]
    Focused,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachmentSurfaceParams {
    source_id: String,
    message_id: String,
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageSurfaceParams {
    conversation_id: String,
    source_id: String,
    message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsSurfaceParams {
    category: Option<SettingsSurfaceCategory>,
    target: Option<SettingsSurfaceTarget>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SettingsSurfaceCategory {
    General,
    Appearance,
    Accounts,
    Mailboxes,
}

impl SettingsSurfaceCategory {
    fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Accounts => "accounts",
            Self::Mailboxes => "mailboxes",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ComposeSurfaceParams {
    #[serde(rename = "new")]
    New {
        #[serde(rename = "sourceId")]
        source_id: String,
    },
    #[serde(rename = "reply")]
    Reply {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
    #[serde(rename = "forward")]
    Forward {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum SettingsSurfaceTarget {
    #[serde(rename = "account")]
    Account {
        #[serde(rename = "accountId")]
        account_id: String,
    },
    #[serde(rename = "newAccount")]
    NewAccount,
    #[serde(rename = "smartMailbox")]
    SmartMailbox {
        #[serde(rename = "smartMailboxId")]
        smart_mailbox_id: String,
    },
    #[serde(rename = "newSmartMailbox")]
    NewSmartMailbox,
    #[serde(rename = "sourceMailbox")]
    SourceMailbox {
        #[serde(rename = "sourceAccountId")]
        source_account_id: String,
        #[serde(rename = "sourceMailboxId")]
        source_mailbox_id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrontendLogEntry {
    level: String,
    domain: String,
    message: String,
    event: Option<String>,
    request_id: Option<String>,
    operation_id: Option<String>,
    operation_kind: Option<String>,
    operation_source: Option<String>,
    session_id: Option<String>,
}

/// Receive a log entry from the frontend and emit it through the backend's
/// tracing subscriber so it lands in the same log files with rotation.
#[tauri::command]
fn log_from_frontend(entry: FrontendLogEntry) {
    let level = entry.level.as_str();
    let domain = entry.domain.as_str();
    let message = entry.message.as_str();
    let request_id = log_token(entry.request_id);
    let event = log_token(entry.event);
    let operation_id = log_token(entry.operation_id);
    let operation_kind = log_token(entry.operation_kind);
    let operation_source = log_token(entry.operation_source);
    let session_id = log_token(entry.session_id);
    let process_id = std::process::id();
    match level {
        "error" => ph_forwarded_error!(
            target: "frontend",
            event: event.as_str(),
            source = "frontend",
            process_id,
            process_role = "webview",
            domain,
            request_id = request_id.as_str(),
            operation_id = operation_id.as_str(),
            operation_kind = operation_kind.as_str(),
            operation_source = operation_source.as_str(),
            session_id = session_id.as_str(),
            "{message}"
        ),
        "warn" => ph_forwarded_warn!(
            target: "frontend",
            event: event.as_str(),
            source = "frontend",
            process_id,
            process_role = "webview",
            domain,
            request_id = request_id.as_str(),
            operation_id = operation_id.as_str(),
            operation_kind = operation_kind.as_str(),
            operation_source = operation_source.as_str(),
            session_id = session_id.as_str(),
            "{message}"
        ),
        "info" => ph_forwarded_info!(
            target: "frontend",
            event: event.as_str(),
            source = "frontend",
            process_id,
            process_role = "webview",
            domain,
            request_id = request_id.as_str(),
            operation_id = operation_id.as_str(),
            operation_kind = operation_kind.as_str(),
            operation_source = operation_source.as_str(),
            session_id = session_id.as_str(),
            "{message}"
        ),
        "debug" => ph_forwarded_debug!(
            target: "frontend",
            event: event.as_str(),
            source = "frontend",
            process_id,
            process_role = "webview",
            domain,
            request_id = request_id.as_str(),
            operation_id = operation_id.as_str(),
            operation_kind = operation_kind.as_str(),
            operation_source = operation_source.as_str(),
            session_id = session_id.as_str(),
            "{message}"
        ),
        _ => ph_forwarded_trace!(
            target: "frontend",
            event: event.as_str(),
            source = "frontend",
            process_id,
            process_role = "webview",
            domain,
            request_id = request_id.as_str(),
            operation_id = operation_id.as_str(),
            operation_kind = operation_kind.as_str(),
            operation_source = operation_source.as_str(),
            session_id = session_id.as_str(),
            "{message}"
        ),
    }
}

fn log_token(value: Option<String>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || !value.chars().all(is_safe_log_token) {
        return String::new();
    }
    value.to_string()
}

fn is_safe_log_token(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.' | ':')
}

#[tauri::command]
fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    validate_external_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_surface_window(app: AppHandle, surface: SurfaceDescriptor) -> Result<(), String> {
    validate_surface_descriptor(&surface)?;
    let label = surface_window_label(&surface);
    let route = surface_route(&surface);
    if let Some(window) = app.get_webview_window(&label) {
        window
            .eval(surface_window_navigation_script(&route))
            .map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    #[cfg(feature = "embedded-server")]
    let backend = {
        let backend = app.state::<EmbeddedBackend>();
        BackendInjection {
            port: backend.port,
            auth_token: backend.auth_token.clone(),
        }
    };
    #[cfg(not(feature = "embedded-server"))]
    let backend = BackendInjection::none();
    let title = surface_title(&surface);
    let (width, height) = surface_window_size(&surface);
    build_window(
        &app,
        &label,
        &format!("index.html#{route}"),
        title,
        width,
        height,
        &backend,
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "embedded-server")]
struct EmbeddedBackend {
    port: u16,
    auth_token: String,
}

/// Backend connection details injected into a webview at window-build time.
///
/// In the embedded build this carries the in-process server's port and token,
/// which are injected as `window.__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__`. In
/// the client-only build (`embedded-server` off) it carries nothing and no
/// injection occurs; the connection-profile runtime (Phase B) supplies the
/// backend in that mode.
struct BackendInjection {
    #[cfg_attr(not(feature = "embedded-server"), allow(dead_code))]
    port: u16,
    #[cfg_attr(not(feature = "embedded-server"), allow(dead_code))]
    auth_token: String,
}

impl BackendInjection {
    #[cfg(not(feature = "embedded-server"))]
    fn none() -> Self {
        Self {
            port: 0,
            auth_token: String::new(),
        }
    }
}

struct FocusedWindowLabel {
    label: Mutex<String>,
}

impl FocusedWindowLabel {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: Mutex::new(label.into()),
        }
    }

    fn get(&self) -> String {
        self.label
            .lock()
            .expect("focused label lock poisoned")
            .clone()
    }

    fn set(&self, label: impl Into<String>) {
        *self.label.lock().expect("focused label lock poisoned") = label.into();
    }
}

/// Run the Posthaste desktop application.
///
/// Starts the embedded Axum backend on an OS-assigned port, then opens a Tauri
/// webview. The port is injected into the webview via `initialization_script`
/// as `window.__POSTHASTE_PORT__` so the frontend can discover the backend.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());

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
        client_connection::client_connections_read,
        client_connection::client_connections_write,
        client_connection::client_token_get,
        client_connection::client_token_set,
        client_connection::client_token_delete,
        client_connection::client_local_daemon_read,
        e2e::posthaste_e2e_result
    ]);
    #[cfg(not(feature = "e2e-testing"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        log_from_frontend,
        open_external_url,
        open_surface_window,
        toggle_devtools,
        client_connection::client_connections_read,
        client_connection::client_connections_write,
        client_connection::client_token_get,
        client_connection::client_token_set,
        client_connection::client_token_delete,
        client_connection::client_local_daemon_read
    ]);

    let builder = builder.setup(|app| {
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

fn build_app_menu<M: Manager<R>, R: Runtime>(manager: &M) -> tauri::Result<Menu<R>> {
    // Devtools are no longer a native menu item; they are toggled from the web
    // (Cmd/Ctrl+Alt+I) gated by the "Developer tools" setting via the
    // `toggle_devtools` command, so they can be flipped on/off in one build.

    // macOS: build the standard App / Edit / Window submenus out of predefined
    // items. Predefined items map to native AppKit selectors (`performClose:`,
    // `copy:`, …) dispatched through the responder chain, so their key
    // equivalents fire even while the WKWebView holds focus. A custom MenuItem
    // accelerator (the route used on other platforms below) is swallowed by the
    // focused webview, which is why Cmd+W and the other standard shortcuts were
    // dead. `close_window` -> `performClose:` closes the focused window for all
    // windows uniformly, replacing the bespoke close-routing used elsewhere.
    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(manager, manager.package_info().name.clone())
            .about(None)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;
        let edit_menu = SubmenuBuilder::new(manager, "Edit")
            .undo()
            .redo()
            .separator()
            .cut()
            .copy()
            .paste()
            .select_all()
            .build()?;
        let window_menu = SubmenuBuilder::new(manager, "Window")
            .minimize()
            .maximize()
            .separator()
            .close_window()
            .build()?;

        let builder = MenuBuilder::new(manager).item(&app_menu).item(&edit_menu);
        let builder = builder.item(&window_menu);
        return builder.build();
    }

    // Other platforms keep the custom Close Window item: their webviews do not
    // intercept the accelerator the way the macOS WKWebView does, and the
    // predefined close item is macOS-only.
    #[cfg(not(target_os = "macos"))]
    {
        let close_window = tauri::menu::MenuItem::with_id(
            manager,
            CLOSE_WINDOW_MENU_ID,
            "Close Window",
            true,
            Some("CmdOrCtrl+W"),
        )?;
        let file_menu = SubmenuBuilder::new(manager, "File")
            .item(&close_window)
            .build()?;
        let builder = MenuBuilder::new(manager).item(&file_menu);
        builder.build()
    }
}

fn is_main_window_label(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL
}

fn is_closeable_surface_window_label(label: &str) -> bool {
    label == "settings"
        || label.starts_with("message-")
        || label.starts_with("attachment-")
        || label.starts_with("compose-")
}

fn remember_focused_window<R: Runtime>(window: &WebviewWindow<R>) {
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    app.state::<FocusedWindowLabel>().set(label.clone());
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(true)) {
            app.state::<FocusedWindowLabel>().set(label.clone());
        }
    });
}

fn close_remembered_webview_window<R: Runtime>(app: &AppHandle<R>) {
    let remembered_label = app.state::<FocusedWindowLabel>().get();
    if request_close_for_window_label(app, &remembered_label) {
        return;
    }

    if let Some(window) = app
        .webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false))
    {
        let label = window.label().to_string();
        app.state::<FocusedWindowLabel>().set(label.clone());
        let _ = request_close_for_window_label(app, &label);
    }
}

/// Open or close the calling window's devtools. The `devtools` feature is
/// compiled into release builds, so this is the runtime hook for the
/// "Developer tools" setting: the web side only invokes it when that flip is on
/// (and binds the shortcut), giving a single build with toggleable devtools
/// instead of a separate DevTools bundle. A no-op when devtools are not
/// compiled in.
#[tauri::command]
fn toggle_devtools(_window: tauri::WebviewWindow) {
    #[cfg(any(debug_assertions, feature = "devtools"))]
    {
        if _window.is_devtools_open() {
            _window.close_devtools();
        } else {
            _window.open_devtools();
        }
    }
}

fn request_close_for_window_label<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    if is_main_window_label(label) {
        let _ = app.emit_to(
            EventTarget::webview_window(MAIN_WINDOW_LABEL),
            CLOSE_WINDOW_REQUESTED_EVENT,
            (),
        );
        return true;
    }

    if !is_closeable_surface_window_label(label) {
        return false;
    }

    let Some(window) = app.get_webview_window(label) else {
        return false;
    };
    let _ = window.close();
    true
}

// The window factory threads through the discrete webview parameters
// (geometry, backend injection); grouping the geometry into a struct would add
// indirection without clarity for a single internal helper.
#[allow(clippy::too_many_arguments)]
fn build_window<M: Manager<R>, R: Runtime>(
    manager: &M,
    label: &str,
    path: &str,
    title: &str,
    width: f64,
    height: f64,
    backend: &BackendInjection,
) -> tauri::Result<WebviewWindow<R>> {
    let builder = WebviewWindowBuilder::new(manager, label, WebviewUrl::App(path.into()))
        .initialization_script(backend_init_script(backend, label))
        .title(title)
        .inner_size(width, height)
        .resizable(true);
    // Every window inherits the inset/overlay macOS title bar so the traffic
    // lights ("semaphore") sit inside the app UI; the web shell paints the
    // matching drag region + inset. Keep this position in sync with
    // WINDOW_TITLEBAR_HEIGHT / WINDOW_TRAFFIC_LIGHT_INSET in
    // apps/web/src/components/WindowChrome.tsx.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 15.0));

    let window = builder.build()?;
    remember_focused_window(&window);
    Ok(window)
}

fn validate_external_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "external URL is invalid".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err("external URL must use http or https".to_string()),
    }
}

fn backend_init_script(backend: &BackendInjection, window_label: &str) -> String {
    let window_label_json =
        serde_json::to_string(window_label).expect("window label should serialize to JSON");
    let window_label_script = format!(
        "Object.defineProperty(window, '__POSTHASTE_WINDOW_LABEL__', {{ value: {window_label_json}, writable: false }});"
    );
    // Embedded build: inject the in-process server's port + token so the
    // frontend client resolves the backend at module load. Client-only build:
    // skip injection entirely — `desktop.ts` degrades and the connection-profile
    // runtime (Phase B) supplies the backend.
    #[cfg(feature = "embedded-server")]
    let script = {
        let port = backend.port;
        // JSON-encode the token so it is safely quoted/escaped in the JS string.
        let auth_token_json = serde_json::to_string(&backend.auth_token)
            .expect("auth token should serialize to JSON");
        format!(
            "Object.defineProperty(window, '__POSTHASTE_PORT__', {{ value: {port}, writable: false }});\
             Object.defineProperty(window, '__POSTHASTE_TOKEN__', {{ value: {auth_token_json}, writable: false }});\
             {window_label_script}"
        )
    };
    #[cfg(not(feature = "embedded-server"))]
    let script = {
        let _ = backend;
        window_label_script
    };
    #[cfg(feature = "e2e-testing")]
    {
        let mut script = script;
        script.push_str(e2e::bridge_initialization_script());
        script
    }
    #[cfg(not(feature = "e2e-testing"))]
    {
        script
    }
}

fn surface_route(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Attachment {
            disposition,
            params,
        } => {
            let _ = disposition;
            format!(
                "/surface/attachment?sourceId={}&messageId={}&attachmentId={}",
                encode_component(&params.source_id),
                encode_component(&params.message_id),
                encode_component(&params.attachment_id)
            )
        }
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
            push_query_pair(
                &mut pairs,
                "category",
                params
                    .category
                    .as_ref()
                    .map(SettingsSurfaceCategory::as_str),
            );
            push_settings_target_query_pairs(&mut pairs, params.target.as_ref());
            if pairs.is_empty() {
                "/surface/settings".to_string()
            } else {
                format!("/surface/settings?{}", pairs.join("&"))
            }
        }
        SurfaceDescriptor::Compose {
            disposition,
            params,
        } => {
            let _ = disposition;
            let mut pairs = Vec::new();
            push_compose_query_pairs(&mut pairs, params);
            format!("/surface/compose?{}", pairs.join("&"))
        }
    }
}

fn surface_window_navigation_script(route: &str) -> String {
    let route_json = serde_json::to_string(route).expect("surface route should serialize to JSON");
    format!(
        "(() => {{ const route = {route_json}; window.history.replaceState(window.history.state, '', '#' + route); window.dispatchEvent(new HashChangeEvent('hashchange')); }})();"
    )
}

fn validate_surface_descriptor(surface: &SurfaceDescriptor) -> Result<(), String> {
    if let SurfaceDescriptor::Settings { params, .. } = surface {
        if let (Some(category), Some(target)) = (&params.category, &params.target) {
            let target_category = settings_target_category(target);
            if *category != target_category {
                return Err("settings surface category does not match target kind".to_string());
            }
        }
    }
    Ok(())
}

fn settings_target_category(target: &SettingsSurfaceTarget) -> SettingsSurfaceCategory {
    match target {
        SettingsSurfaceTarget::Account { .. } | SettingsSurfaceTarget::NewAccount => {
            SettingsSurfaceCategory::Accounts
        }
        SettingsSurfaceTarget::SmartMailbox { .. }
        | SettingsSurfaceTarget::NewSmartMailbox
        | SettingsSurfaceTarget::SourceMailbox { .. } => SettingsSurfaceCategory::Mailboxes,
    }
}

fn surface_window_label(surface: &SurfaceDescriptor) -> String {
    match surface {
        SurfaceDescriptor::Attachment { params, .. } => {
            let key = format!(
                "{}:{}:{}",
                params.source_id, params.message_id, params.attachment_id
            );
            format!("attachment-{:016x}", stable_hash(key.as_bytes()))
        }
        SurfaceDescriptor::Settings { .. } => "settings".to_string(),
        SurfaceDescriptor::Compose { .. } => {
            format!(
                "compose-{:016x}",
                stable_hash(surface_route(surface).as_bytes())
            )
        }
        SurfaceDescriptor::Message { params, .. } => {
            let key = format!("{}:{}", params.source_id, params.message_id);
            format!("message-{:016x}", stable_hash(key.as_bytes()))
        }
    }
}

fn surface_title(surface: &SurfaceDescriptor) -> &'static str {
    match surface {
        SurfaceDescriptor::Attachment { .. } => "Posthaste Attachment",
        SurfaceDescriptor::Settings { .. } => "Posthaste Settings",
        SurfaceDescriptor::Message { .. } => "Posthaste Message",
        SurfaceDescriptor::Compose { .. } => "Posthaste Compose",
    }
}

fn surface_window_size(surface: &SurfaceDescriptor) -> (f64, f64) {
    match surface {
        SurfaceDescriptor::Attachment { .. } => (1100.0, 820.0),
        SurfaceDescriptor::Settings { .. } => (980.0, 720.0),
        SurfaceDescriptor::Message { .. } => (900.0, 760.0),
        SurfaceDescriptor::Compose { .. } => (780.0, 640.0),
    }
}

fn push_query_pair(pairs: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        pairs.push(format!("{key}={}", encode_component(value)));
    }
}

fn push_compose_query_pairs(pairs: &mut Vec<String>, params: &ComposeSurfaceParams) {
    match params {
        ComposeSurfaceParams::New { source_id } => {
            push_query_pair(pairs, "composeKind", Some("new"));
            push_query_pair(pairs, "sourceId", Some(source_id));
        }
        ComposeSurfaceParams::Reply {
            source_id,
            message_id,
        } => {
            push_query_pair(pairs, "composeKind", Some("reply"));
            push_query_pair(pairs, "sourceId", Some(source_id));
            push_query_pair(pairs, "messageId", Some(message_id));
        }
        ComposeSurfaceParams::Forward {
            source_id,
            message_id,
        } => {
            push_query_pair(pairs, "composeKind", Some("forward"));
            push_query_pair(pairs, "sourceId", Some(source_id));
            push_query_pair(pairs, "messageId", Some(message_id));
        }
    }
}

fn push_settings_target_query_pairs(
    pairs: &mut Vec<String>,
    target: Option<&SettingsSurfaceTarget>,
) {
    let Some(target) = target else {
        return;
    };

    match target {
        SettingsSurfaceTarget::Account { account_id } => {
            push_query_pair(pairs, "targetKind", Some("account"));
            push_query_pair(pairs, "accountId", Some(account_id));
        }
        SettingsSurfaceTarget::NewAccount => {
            push_query_pair(pairs, "targetKind", Some("newAccount"));
        }
        SettingsSurfaceTarget::SmartMailbox { smart_mailbox_id } => {
            push_query_pair(pairs, "targetKind", Some("smartMailbox"));
            push_query_pair(pairs, "smartMailboxId", Some(smart_mailbox_id));
        }
        SettingsSurfaceTarget::NewSmartMailbox => {
            push_query_pair(pairs, "targetKind", Some("newSmartMailbox"));
        }
        SettingsSurfaceTarget::SourceMailbox {
            source_account_id,
            source_mailbox_id,
        } => {
            push_query_pair(pairs, "targetKind", Some("sourceMailbox"));
            push_query_pair(pairs, "sourceAccountId", Some(source_account_id));
            push_query_pair(pairs, "sourceMailboxId", Some(source_mailbox_id));
        }
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
    fn attachment_surface_route_uses_hash_route_and_encoded_params() {
        let surface = SurfaceDescriptor::Attachment {
            disposition: SurfaceDisposition::Focused,
            params: AttachmentSurfaceParams {
                source_id: "source:primary".to_string(),
                message_id: "message 1".to_string(),
                attachment_id: "part/2".to_string(),
            },
        };

        assert_eq!(
            surface_route(&surface),
            "/surface/attachment?sourceId=source%3Aprimary&messageId=message%201&attachmentId=part%2F2"
        );
    }

    #[test]
    fn closeable_window_labels_distinguish_main_and_surface_windows() {
        assert!(is_main_window_label("main"));
        assert!(!is_main_window_label("settings"));
        assert!(!is_closeable_surface_window_label("main"));
        assert!(is_closeable_surface_window_label("settings"));
        assert!(is_closeable_surface_window_label(
            "message-0123456789abcdef"
        ));
        assert!(is_closeable_surface_window_label(
            "attachment-0123456789abcdef"
        ));
        assert!(is_closeable_surface_window_label(
            "compose-0123456789abcdef"
        ));
    }

    #[test]
    fn compose_surface_descriptor_deserializes_frontend_camel_case_params() {
        let surface: SurfaceDescriptor = serde_json::from_value(serde_json::json!({
            "kind": "compose",
            "disposition": "focused",
            "params": {
                "kind": "reply",
                "sourceId": "source:primary",
                "messageId": "message 1"
            }
        }))
        .unwrap();

        assert_eq!(
            surface_route(&surface),
            "/surface/compose?composeKind=reply&sourceId=source%3Aprimary&messageId=message%201"
        );
    }

    #[test]
    fn compose_surface_route_uses_hash_route_and_encoded_params() {
        let surface = SurfaceDescriptor::Compose {
            disposition: SurfaceDisposition::Focused,
            params: ComposeSurfaceParams::Reply {
                source_id: "source:primary".to_string(),
                message_id: "message 1".to_string(),
            },
        };

        assert_eq!(
            surface_route(&surface),
            "/surface/compose?composeKind=reply&sourceId=source%3Aprimary&messageId=message%201"
        );
        assert!(surface_window_label(&surface).starts_with("compose-"));
        assert_eq!(surface_title(&surface), "Posthaste Compose");
        assert_eq!(surface_window_size(&surface), (780.0, 640.0));
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
    fn settings_window_navigation_script_replaces_hash_route() {
        let script = surface_window_navigation_script("/surface/settings?category=accounts");

        assert!(script.contains("window.history.replaceState"));
        assert!(script.contains("window.dispatchEvent(new HashChangeEvent('hashchange'))"));
        assert!(script.contains("\"/surface/settings?category=accounts\""));
    }

    #[test]
    fn settings_surface_descriptor_deserializes_frontend_camel_case_target() {
        let surface = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
            "kind": "settings",
            "disposition": "focused",
            "params": {
                "category": "mailboxes",
                "target": {
                    "kind": "sourceMailbox",
                    "sourceAccountId": "primary",
                    "sourceMailboxId": "inbox"
                }
            }
        }))
        .expect("frontend settings surface descriptors should deserialize");

        assert_eq!(
            surface_route(&surface),
            "/surface/settings?category=mailboxes&targetKind=sourceMailbox&sourceAccountId=primary&sourceMailboxId=inbox"
        );
        assert!(validate_surface_descriptor(&surface).is_ok());
    }

    #[test]
    fn settings_surface_rejects_unknown_frontend_category() {
        let result = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
            "kind": "settings",
            "disposition": "focused",
            "params": {
                "category": "advanced"
            }
        }));

        assert!(result.is_err());
    }

    #[test]
    fn surface_descriptors_reject_unknown_frontend_fields() {
        let result = serde_json::from_value::<SurfaceDescriptor>(serde_json::json!({
            "kind": "compose",
            "disposition": "focused",
            "params": {
                "kind": "new",
                "sourceId": "primary",
                "draftId": "unexpected"
            }
        }));

        assert!(result.is_err());
    }

    #[test]
    fn settings_surface_category_must_match_target_kind() {
        let surface = SurfaceDescriptor::Settings {
            disposition: SurfaceDisposition::Focused,
            params: SettingsSurfaceParams {
                category: Some(SettingsSurfaceCategory::Mailboxes),
                target: Some(SettingsSurfaceTarget::Account {
                    account_id: "primary".to_string(),
                }),
            },
        };

        assert_eq!(
            validate_surface_descriptor(&surface).unwrap_err(),
            "settings surface category does not match target kind"
        );
    }

    #[test]
    fn settings_window_reuses_stable_label() {
        let surface = SurfaceDescriptor::Settings {
            disposition: SurfaceDisposition::Focused,
            params: SettingsSurfaceParams {
                category: Some(SettingsSurfaceCategory::Accounts),
                target: Some(SettingsSurfaceTarget::Account {
                    account_id: "primary".to_string(),
                }),
            },
        };

        assert_eq!(surface_window_label(&surface), "settings");
        assert_eq!(
            surface_route(&surface),
            "/surface/settings?category=accounts&targetKind=account&accountId=primary"
        );
    }

    #[test]
    fn external_url_validation_accepts_http_urls() {
        assert!(validate_external_url("https://accounts.example.test/oauth").is_ok());
        assert!(validate_external_url("http://127.0.0.1/callback").is_ok());
    }

    #[test]
    fn external_url_validation_rejects_non_web_urls() {
        assert!(validate_external_url("file:///tmp/secret").is_err());
        assert!(validate_external_url("not a url").is_err());
    }

    #[test]
    fn log_token_accepts_short_ascii_metadata() {
        assert_eq!(
            log_token(Some(" mail.search:preview_1 ".to_string())),
            "mail.search:preview_1"
        );
    }

    #[test]
    fn log_token_rejects_unsafe_metadata() {
        assert_eq!(log_token(Some("mail search".to_string())), "");
        assert_eq!(log_token(Some("mail/search".to_string())), "");
        assert_eq!(log_token(Some("x".repeat(129))), "");
    }
}
