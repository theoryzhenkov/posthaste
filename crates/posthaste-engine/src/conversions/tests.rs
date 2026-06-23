use serde_json::json;

use super::*;

#[test]
fn message_record_prefers_sent_at_over_server_received_at() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "message-1",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z",
        "sentAt": "2026-03-01T12:34:56Z"
    }))
    .expect("deserialize JMAP email");

    assert_eq!(
        to_message_record(&email).received_at,
        "2026-03-01T12:34:56Z"
    );
}

#[test]
fn message_record_falls_back_to_received_at_when_sent_at_is_missing() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "message-1",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z"
    }))
    .expect("deserialize JMAP email");

    assert_eq!(
        to_message_record(&email).received_at,
        "2026-04-22T19:04:20Z"
    );
}

#[test]
fn message_record_reads_the_draft_id_header() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "draft-1",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z",
        "header:X-Posthaste-Draft-Id:asText": "draft-local-stable"
    }))
    .expect("deserialize JMAP email");

    assert_eq!(
        to_message_record(&email).draft_id.as_deref(),
        Some("draft-local-stable")
    );
}

#[test]
fn message_record_has_no_draft_id_without_the_header() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "message-1",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z"
    }))
    .expect("deserialize JMAP email");

    assert_eq!(to_message_record(&email).draft_id, None);
}
