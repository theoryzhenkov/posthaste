//! Connection-info injection: every webview gets the embedded backend's port
//! and session token as window globals via an initialization script, before
//! the page loads. The frontend's client bootstrap reads them and falls back
//! to dev-proxy behavior (same-origin, no token) when they are absent.

use std::sync::Mutex;

/// Backend connection details injected into a webview at window-build time,
/// as `window.__POSTHASTE_PORT__` / `window.__POSTHASTE_TOKEN__`.
pub(crate) struct BackendInjection {
    pub(crate) port: u16,
    pub(crate) auth_token: String,
}

/// Label of the most recently focused window, so the Close Window menu item
/// can route to it.
pub(crate) struct FocusedWindowLabel {
    label: Mutex<String>,
}

impl FocusedWindowLabel {
    pub(crate) fn new(label: impl Into<String>) -> Self {
        Self {
            label: Mutex::new(label.into()),
        }
    }

    pub(crate) fn get(&self) -> String {
        self.label
            .lock()
            .expect("focused label lock poisoned")
            .clone()
    }

    pub(crate) fn set(&self, label: impl Into<String>) {
        *self.label.lock().expect("focused label lock poisoned") = label.into();
    }
}

pub(crate) fn backend_init_script(backend: &BackendInjection, window_label: &str) -> String {
    let window_label_json =
        serde_json::to_string(window_label).expect("window label should serialize to JSON");
    let port = backend.port;
    // JSON-encode the token so it is safely quoted/escaped in the JS string.
    let auth_token_json =
        serde_json::to_string(&backend.auth_token).expect("auth token should serialize to JSON");
    format!(
        "Object.defineProperty(window, '__POSTHASTE_RUNTIME_MODE__', {{ value: 'loopback', writable: false }});\
         Object.defineProperty(window, '__POSTHASTE_PORT__', {{ value: {port}, writable: false }});\
         Object.defineProperty(window, '__POSTHASTE_TOKEN__', {{ value: {auth_token_json}, writable: false }});\
         Object.defineProperty(window, '__POSTHASTE_WINDOW_LABEL__', {{ value: {window_label_json}, writable: false }});"
    )
}
