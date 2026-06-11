use axum::http::HeaderMap;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: &str = "x-posthaste-request-id";
pub const OPERATION_ID_HEADER: &str = "x-posthaste-operation-id";
pub const OPERATION_KIND_HEADER: &str = "x-posthaste-operation-kind";
pub const OPERATION_SOURCE_HEADER: &str = "x-posthaste-operation-source";
pub const SESSION_ID_HEADER: &str = "x-posthaste-session-id";

#[derive(Clone, Debug)]
pub struct RequestLogContext {
    pub request_id: String,
    pub operation_id: Option<String>,
    pub operation_kind: Option<String>,
    pub operation_source: Option<String>,
    pub session_id: Option<String>,
}

impl RequestLogContext {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            request_id: header_value(headers, REQUEST_ID_HEADER)
                .unwrap_or_else(|| format!("srv_{}", Uuid::new_v4())),
            operation_id: operation_id_from_headers(headers),
            operation_kind: header_value(headers, OPERATION_KIND_HEADER),
            operation_source: header_value(headers, OPERATION_SOURCE_HEADER),
            session_id: header_value(headers, SESSION_ID_HEADER),
        }
    }
}

pub fn operation_id_from_headers(headers: &HeaderMap) -> Option<String> {
    header_value(headers, OPERATION_ID_HEADER)
}

fn header_value(headers: &HeaderMap, name: &'static str) -> Option<String> {
    let value = headers.get(name)?.to_str().ok()?.trim();
    if value.is_empty() || value.len() > 128 || !value.chars().all(is_safe_log_token) {
        return None;
    }
    Some(value.to_string())
}

fn is_safe_log_token(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.' | ':')
}

#[cfg(test)]
mod tests;
