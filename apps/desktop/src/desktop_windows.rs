use super::*;

#[tauri::command]
pub(crate) fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    validate_external_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn open_surface_window(
    app: AppHandle,
    surface: SurfaceDescriptor,
) -> Result<(), String> {
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
pub(crate) fn is_main_window_label(label: &str) -> bool {
    label == MAIN_WINDOW_LABEL
}

pub(crate) fn is_closeable_surface_window_label(label: &str) -> bool {
    label == "settings"
        || label.starts_with("message-")
        || label.starts_with("attachment-")
        || label.starts_with("compose-")
}

pub(crate) fn remember_focused_window<R: Runtime>(window: &WebviewWindow<R>) {
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    app.state::<FocusedWindowLabel>().set(label.clone());
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(true)) {
            app.state::<FocusedWindowLabel>().set(label.clone());
        }
    });
}

pub(crate) fn close_remembered_webview_window<R: Runtime>(app: &AppHandle<R>) {
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
pub(crate) fn toggle_devtools(_window: tauri::WebviewWindow) {
    #[cfg(any(debug_assertions, feature = "devtools"))]
    {
        if _window.is_devtools_open() {
            _window.close_devtools();
        } else {
            _window.open_devtools();
        }
    }
}

pub(crate) fn request_close_for_window_label<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
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
pub(crate) fn build_window<M: Manager<R>, R: Runtime>(
    manager: &M,
    label: &str,
    path: &str,
    title: &str,
    width: f64,
    height: f64,
    backend: &BackendInjection,
) -> tauri::Result<WebviewWindow<R>> {
    let opener_handle = manager.app_handle().clone();
    let builder = WebviewWindowBuilder::new(manager, label, WebviewUrl::App(path.into()))
        .initialization_script(backend_init_script(backend, label))
        .title(title)
        .inner_size(width, height)
        .resizable(true)
        // Email links navigate the top frame (`<base target="_top">`) because a
        // click handler can't run in the no-scripts sandboxed reader iframe.
        // Open external web URLs in the system browser and block the in-app
        // navigation; the app's own (loopback / tauri) navigations pass through.
        .on_navigation(move |url| {
            if is_external_web_url(url) {
                let _ = opener_handle.opener().open_url(url.as_str(), None::<&str>);
                return false;
            }
            true
        });
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

pub(crate) fn validate_external_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "external URL is invalid".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err("external URL must use http or https".to_string()),
    }
}

/// Whether a navigation target is an external web URL that should open in the
/// system browser rather than navigate the webview. The bundled UI loads from
/// `tauri://localhost` (or the dev loopback) and routes by hash, so any real
/// `http`/`https` navigation to a non-loopback host is an outbound link.
pub(crate) fn is_external_web_url(url: &url::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    !matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1") | Some("tauri.localhost")
    )
}
