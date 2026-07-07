use super::*;
use std::collections::HashSet;

/// Window labels whose webview ACKed frontend boot (`surface_webview_booted`).
///
/// Defense in depth for the close path: closing a window whose frontend never
/// booted must not depend on JS close handling of any kind, so
/// [`request_close_for_window_label`] force-destroys such a window instead of
/// asking it to close gracefully. A booted webview keeps the guarded close
/// flow (the compose close-guard listens to `tauri://close-requested`).
#[derive(Default)]
pub(crate) struct WebviewBootAcks {
    booted: Mutex<HashSet<String>>,
}

impl WebviewBootAcks {
    pub(crate) fn mark_booted(&self, label: impl Into<String>) {
        self.booted
            .lock()
            .expect("boot ack lock poisoned")
            .insert(label.into());
    }

    /// Forget a label when its window is destroyed, so a future window that
    /// reuses the label (e.g. the stable "settings" label) starts un-booted.
    pub(crate) fn clear(&self, label: &str) {
        self.booted
            .lock()
            .expect("boot ack lock poisoned")
            .remove(label);
    }

    pub(crate) fn is_booted(&self, label: &str) -> bool {
        self.booted
            .lock()
            .expect("boot ack lock poisoned")
            .contains(label)
    }
}

/// How to honor a close request for a surface window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SurfaceCloseAction {
    /// The webview booted: request a graceful close so JS close guards
    /// (`tauri://close-requested` listeners, e.g. compose) can intervene.
    CloseGuarded,
    /// The webview never ACKed boot: no JS is listening, destroy the window
    /// outright so a frontend-load failure can never yield an unclosable
    /// window.
    ForceDestroy,
}

pub(crate) fn surface_close_action(webview_booted: bool) -> SurfaceCloseAction {
    if webview_booted {
        SurfaceCloseAction::CloseGuarded
    } else {
        SurfaceCloseAction::ForceDestroy
    }
}

/// Frontend boot ACK, invoked from `main.tsx` as soon as the bundle executes.
/// The window argument is supplied by tauri, so the label cannot be spoofed by
/// the renderer for another window.
#[tauri::command]
pub(crate) fn surface_webview_booted(window: WebviewWindow) {
    window
        .app_handle()
        .state::<WebviewBootAcks>()
        .mark_booted(window.label());
}

#[tauri::command]
pub(crate) fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    validate_external_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

/// Open (or focus + re-route) a standalone surface window.
///
/// MUST stay `async`: on Windows, creating a webview window inside a
/// *synchronous* command deadlocks the app — the sync invoke handler runs on
/// the main thread, `WebviewWindowBuilder::build` blocks that thread waiting
/// for WebView2's async controller creation, and the completion is delivered
/// via the same thread's message pump (tauri `webview/webview_window.rs`
/// "Known issues" on `WebviewWindowBuilder::new`/`build`; wry issue #583).
/// The symptom was the v0.4.0/v0.5.0 Windows bug: a black surface window with
/// no webview attached, an event loop too wedged to process the titlebar X,
/// and zero frontend JS. An `async` command runs on the async runtime thread
/// pool, leaving the main thread free to pump the creation to completion.
#[tauri::command]
pub(crate) async fn open_surface_window(
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
        &surface_window_url(&surface),
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
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            app.state::<FocusedWindowLabel>().set(label.clone());
        }
        WindowEvent::Destroyed => {
            app.state::<WebviewBootAcks>().clear(&label);
        }
        _ => {}
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
// `_window` is underscored because it is only used when devtools are compiled
// in; under the `not(...)` cfg the body is empty and the argument is unused.
#[allow(clippy::used_underscore_binding)]
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
    let webview_booted = app.state::<WebviewBootAcks>().is_booted(label);

    if is_main_window_label(label) {
        if !webview_booted {
            // No JS ever ran in the main webview, so nothing listens for the
            // guarded close event — emitting it would be a no-op. Close the
            // window natively instead.
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.close();
            }
            return true;
        }
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
    match surface_close_action(webview_booted) {
        SurfaceCloseAction::CloseGuarded => {
            let _ = window.close();
        }
        SurfaceCloseAction::ForceDestroy => {
            let _ = window.destroy();
        }
    }
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
