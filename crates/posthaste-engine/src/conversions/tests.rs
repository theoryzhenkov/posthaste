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

#[test]
fn message_record_parses_the_list_unsubscribe_headers() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "newsletter-1",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z",
        "header:List-Unsubscribe":
            "<https://news.example.com/unsub/opaque>, <mailto:unsub@example.com?subject=stop>",
        "header:List-Unsubscribe-Post": "List-Unsubscribe=One-Click"
    }))
    .expect("deserialize JMAP email");

    let parsed = to_message_record(&email)
        .list_unsubscribe
        .expect("targets parsed");
    assert_eq!(
        parsed.https.as_deref(),
        Some("https://news.example.com/unsub/opaque")
    );
    assert_eq!(
        parsed.mailto.as_deref(),
        Some("mailto:unsub@example.com?subject=stop")
    );
    assert!(parsed.one_click);
}

#[test]
fn message_record_without_post_header_is_not_one_click() {
    let email: jmap_client::email::Email = serde_json::from_value(json!({
        "id": "newsletter-2",
        "threadId": "thread-1",
        "receivedAt": "2026-04-22T19:04:20Z",
        "header:List-Unsubscribe": "<https://news.example.com/unsub/opaque>"
    }))
    .expect("deserialize JMAP email");

    let parsed = to_message_record(&email)
        .list_unsubscribe
        .expect("targets parsed");
    assert!(!parsed.one_click);
}

#[test]
fn message_record_has_no_unsubscribe_without_the_header_or_with_junk() {
    for header in [
        None,
        Some("no angle brackets"),
        Some("<http://insecure.example.com/u>"),
    ] {
        let mut fixture = json!({
            "id": "message-1",
            "threadId": "thread-1",
            "receivedAt": "2026-04-22T19:04:20Z"
        });
        if let Some(header) = header {
            fixture["header:List-Unsubscribe"] = json!(header);
        }
        let email: jmap_client::email::Email =
            serde_json::from_value(fixture).expect("deserialize JMAP email");
        assert_eq!(
            to_message_record(&email).list_unsubscribe,
            None,
            "header: {header:?}"
        );
    }
}
