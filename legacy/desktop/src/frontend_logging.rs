use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontendLogEntry {
    pub(crate) level: String,
    pub(crate) domain: String,
    pub(crate) message: String,
    pub(crate) event: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) operation_id: Option<String>,
    pub(crate) operation_kind: Option<String>,
    pub(crate) operation_source: Option<String>,
    pub(crate) session_id: Option<String>,
}

/// Receive a log entry from the frontend and emit it through the backend's
/// tracing subscriber so it lands in the same log files with rotation.
#[tauri::command]
pub(crate) fn log_from_frontend(entry: FrontendLogEntry) {
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

pub(crate) fn log_token(value: Option<String>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || !value.chars().all(is_safe_log_token) {
        return String::new();
    }
    value.to_string()
}

pub(crate) fn is_safe_log_token(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.' | ':')
}
