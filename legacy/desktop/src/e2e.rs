#[path = "e2e_script.rs"]
mod script;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime, State};

use crate::MAIN_WINDOW_LABEL;

const DEFAULT_TAURI_PLAYWRIGHT_SOCKET: &str = "/tmp/tauri-playwright.sock";
const E2E_COMMAND_EVENT: &str = "posthaste://e2e-command";
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const COMMAND_TIMEOUT_MARGIN_MS: u64 = 2_000;

static NEXT_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct E2eBridgeState {
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<E2eBridgeResponse>>>>,
}

#[derive(Debug, Serialize, Clone)]
struct E2eBridgeResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl E2eBridgeResponse {
    fn ok(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    fn err(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
struct E2eCommandPayload {
    id: String,
    command: Value,
}

#[tauri::command]
pub fn posthaste_e2e_result(
    state: State<'_, E2eBridgeState>,
    id: String,
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
) -> Result<(), String> {
    let sender = state
        .pending
        .lock()
        .map_err(|_| "e2e bridge pending lock poisoned".to_string())?
        .remove(&id);

    if let Some(sender) = sender {
        let response = if ok {
            E2eBridgeResponse::ok(data.unwrap_or(Value::Null))
        } else {
            E2eBridgeResponse::err(error.unwrap_or_else(|| "unknown e2e error".to_string()))
        };
        let _ = sender.send(response);
    }

    Ok(())
}

pub fn start_e2e_bridge<R: Runtime>(app: AppHandle<R>) {
    let socket_path = required_socket_path();
    let pending = app.state::<E2eBridgeState>().inner().pending.clone();

    std::thread::spawn(move || {
        if let Err(error) = run_socket_server(app, pending, &socket_path) {
            eprintln!("tauri-plugin-playwright: unix server error: {error}");
        }
    });
}

fn required_socket_path() -> String {
    let socket_path = std::env::var("POSTHASTE_E2E_SOCKET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .expect(
            "POSTHASTE_E2E_SOCKET must be set to a non-empty private per-run Unix socket path \
             when the e2e-testing feature is enabled",
        );
    if socket_path == DEFAULT_TAURI_PLAYWRIGHT_SOCKET {
        panic!(
            "POSTHASTE_E2E_SOCKET must be a private per-run socket path, not \
             /tmp/tauri-playwright.sock"
        );
    }

    let run_dir = std::env::var("POSTHASTE_LAB_RUN_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .expect(
            "POSTHASTE_LAB_RUN_DIR must be set when the e2e-testing feature is enabled so \
             the private socket can be scoped to a single Lab run",
        );
    let socket_path_buf = PathBuf::from(&socket_path);
    let run_dir_buf = PathBuf::from(&run_dir);
    if !socket_path_buf.is_absolute() || !run_dir_buf.is_absolute() {
        panic!("POSTHASTE_E2E_SOCKET and POSTHASTE_LAB_RUN_DIR must be absolute paths");
    }
    if !socket_path_buf.starts_with(&run_dir_buf) {
        panic!("POSTHASTE_E2E_SOCKET must be inside POSTHASTE_LAB_RUN_DIR");
    }

    socket_path
}

fn run_socket_server<R: Runtime>(
    app: AppHandle<R>,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<E2eBridgeResponse>>>>,
    socket_path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    eprintln!("tauri-plugin-playwright: listening on unix:{socket_path}");

    for stream in listener.incoming() {
        let stream = stream?;
        let app = app.clone();
        let pending = pending.clone();
        std::thread::spawn(move || handle_connection(app, pending, stream));
    }

    Ok(())
}

fn handle_connection<R: Runtime>(
    app: AppHandle<R>,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<E2eBridgeResponse>>>>,
    mut stream: UnixStream,
) {
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            let _ = writeln!(
                stream,
                "{}",
                response_json(E2eBridgeResponse::err(error.to_string()))
            );
            return;
        }
    };
    let reader = BufReader::new(reader_stream);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(line) {
            Ok(command) => execute_command(&app, &pending, command),
            Err(error) => E2eBridgeResponse::err(format!("invalid command: {error}")),
        };

        if writeln!(stream, "{}", response_json(response)).is_err() {
            break;
        }
        if stream.flush().is_err() {
            break;
        }
    }
}

fn response_json(response: E2eBridgeResponse) -> String {
    serde_json::to_string(&response)
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialize error"}"#.to_string())
}

fn execute_command<R: Runtime>(
    app: &AppHandle<R>,
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<E2eBridgeResponse>>>>,
    command: Value,
) -> E2eBridgeResponse {
    let command_type = command
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if command_type == "ping" {
        return E2eBridgeResponse::ok(json!("pong"));
    }

    if matches!(
        command_type,
        "native_screenshot" | "start_recording" | "stop_recording"
    ) {
        return E2eBridgeResponse::err(format!(
            "command '{command_type}' is not supported by the PostHaste Linux e2e bridge"
        ));
    }

    let id = format!("ph{}", NEXT_COMMAND_ID.fetch_add(1, Ordering::SeqCst));
    let (sender, receiver) = mpsc::channel();
    if let Err(error) = pending
        .lock()
        .map_err(|_| "e2e bridge pending lock poisoned".to_string())
        .map(|mut pending| pending.insert(id.clone(), sender))
    {
        return E2eBridgeResponse::err(error);
    }

    let command_timeout = command_timeout(&command);
    let payload = E2eCommandPayload {
        id: id.clone(),
        command,
    };
    if let Err(error) = app.emit_to(
        EventTarget::webview_window(MAIN_WINDOW_LABEL),
        E2E_COMMAND_EVENT,
        payload,
    ) {
        remove_pending(pending, &id);
        return E2eBridgeResponse::err(format!("failed to emit e2e command: {error}"));
    }

    match receiver.recv_timeout(command_timeout) {
        Ok(response) => response,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            remove_pending(pending, &id);
            E2eBridgeResponse::err("timeout waiting for e2e command result")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            remove_pending(pending, &id);
            E2eBridgeResponse::err("e2e command result channel disconnected")
        }
    }
}

fn command_timeout(command: &Value) -> Duration {
    command
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .map(|timeout_ms| {
            Duration::from_millis(timeout_ms.saturating_add(COMMAND_TIMEOUT_MARGIN_MS))
        })
        .unwrap_or(DEFAULT_COMMAND_TIMEOUT)
}

fn remove_pending(
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<E2eBridgeResponse>>>>,
    id: &str,
) {
    if let Ok(mut pending) = pending.lock() {
        pending.remove(id);
    }
}

pub use script::bridge_initialization_script;
