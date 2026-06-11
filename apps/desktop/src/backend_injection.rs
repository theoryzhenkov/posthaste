use super::*;

pub(crate) struct EmbeddedBackend {
    pub(crate) port: u16,
    pub(crate) auth_token: String,
}

/// Backend connection details injected into a webview at window-build time.
///
/// In the embedded build this carries the in-process server's port and token,
/// which are injected as `window.__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__`. In
/// the client-only build (`embedded-server` off) it carries nothing and no
/// injection occurs; the connection-profile runtime (Phase B) supplies the
/// backend in that mode.
pub(crate) struct BackendInjection {
    #[cfg_attr(not(feature = "embedded-server"), allow(dead_code))]
    pub(crate) port: u16,
    #[cfg_attr(not(feature = "embedded-server"), allow(dead_code))]
    pub(crate) auth_token: String,
}

impl BackendInjection {
    #[cfg(not(feature = "embedded-server"))]
    pub(crate) fn none() -> Self {
        Self {
            port: 0,
            auth_token: String::new(),
        }
    }
}

pub(crate) struct FocusedWindowLabel {
    pub(crate) label: Mutex<String>,
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
