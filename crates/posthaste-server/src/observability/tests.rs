use axum::http::HeaderValue;

use super::*;

#[test]
fn request_context_accepts_safe_operation_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("req_123"));
    headers.insert(OPERATION_ID_HEADER, HeaderValue::from_static("op_456"));
    headers.insert(
        OPERATION_KIND_HEADER,
        HeaderValue::from_static("mail.search"),
    );
    headers.insert(
        OPERATION_SOURCE_HEADER,
        HeaderValue::from_static("message-list.smart-mailbox"),
    );
    headers.insert(SESSION_ID_HEADER, HeaderValue::from_static("session_789"));

    let context = RequestLogContext::from_headers(&headers);

    assert_eq!(context.request_id, "req_123");
    assert_eq!(context.operation_id.as_deref(), Some("op_456"));
    assert_eq!(context.operation_kind.as_deref(), Some("mail.search"));
    assert_eq!(
        context.operation_source.as_deref(),
        Some("message-list.smart-mailbox")
    );
    assert_eq!(context.session_id.as_deref(), Some("session_789"));
}

#[test]
fn request_context_rejects_unsafe_header_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        OPERATION_ID_HEADER,
        HeaderValue::from_static("op raw query"),
    );

    let context = RequestLogContext::from_headers(&headers);

    assert!(context.operation_id.is_none());
    assert!(context.request_id.starts_with("srv_"));
}
