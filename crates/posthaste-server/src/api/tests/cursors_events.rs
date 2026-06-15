use super::*;

#[test]
fn conversation_cursor_round_trips() {
    let cursor = ConversationCursor {
        sort_value: "2026-04-01T10:11:12Z".to_string(),
        conversation_id: ConversationId::from("conv-42"),
    };

    let encoded = encode_conversation_cursor(&cursor);
    let decoded = parse_conversation_cursor(Some(&encoded))
        .unwrap_or_else(|_| panic!("cursor should parse"))
        .unwrap_or_else(|| panic!("cursor should be present"));

    assert_eq!(decoded.sort_value, cursor.sort_value);
    assert_eq!(decoded.conversation_id, cursor.conversation_id);
}

#[test]
fn trigger_sync_response_serializes_wire_compatible_json() {
    let body = TriggerSyncResponse {
        ok: true,
        event_count: 3,
        mode: "full".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&body).expect("serialize trigger sync response"),
        json!({ "ok": true, "eventCount": 3, "mode": "full" }),
    );
}

#[test]
fn malformed_conversation_cursor_is_rejected() {
    let error = parse_conversation_cursor(Some("broken-cursor")).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidCursor);
}

#[test]
fn message_cursor_round_trips() {
    let cursor = MessageCursor {
        sort_value: "2026-04-01T10:11:12Z".to_string(),
        source_id: AccountId::from("primary"),
        message_id: MessageId::from("message-42"),
    };

    let encoded = encode_message_cursor(&cursor);
    let decoded = parse_message_cursor(Some(&encoded))
        .unwrap_or_else(|_| panic!("cursor should parse"))
        .unwrap_or_else(|| panic!("cursor should be present"));

    assert_eq!(decoded.sort_value, cursor.sort_value);
    assert_eq!(decoded.source_id, cursor.source_id);
    assert_eq!(decoded.message_id, cursor.message_id);
}

#[test]
fn message_cursor_allows_empty_sort_value() {
    let cursor = MessageCursor {
        sort_value: String::new(),
        source_id: AccountId::from("primary"),
        message_id: MessageId::from("message-42"),
    };

    let encoded = encode_message_cursor(&cursor);
    let decoded = parse_message_cursor(Some(&encoded))
        .unwrap_or_else(|_| panic!("cursor should parse"))
        .unwrap_or_else(|| panic!("cursor should be present"));

    assert_eq!(decoded.sort_value, cursor.sort_value);
    assert_eq!(decoded.source_id, cursor.source_id);
    assert_eq!(decoded.message_id, cursor.message_id);
}

#[test]
fn malformed_message_cursor_is_rejected() {
    let error = parse_message_cursor(Some("broken-cursor")).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidCursor);
}

#[test]
fn invalid_search_query_is_rejected() {
    let error = parse_optional_search_rule(Some("wat:nope")).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidQuery);
}

#[test]
fn source_scope_rule_combines_source_and_mailbox() {
    let rule = source_message_scope_rule("primary", Some(&MailboxId::from("inbox")));

    assert_eq!(rule.root.operator, SmartMailboxGroupOperator::All);
    assert_eq!(rule.root.nodes.len(), 2);
}

#[test]
fn matches_event_applies_all_filters() {
    let event = DomainEvent {
        seq: 5,
        account_id: AccountId::from("primary"),
        topic: EVENT_TOPIC_MESSAGE_ARRIVED.to_string(),
        occurred_at: "2026-03-31T10:00:00Z".to_string(),
        mailbox_id: Some(MailboxId::from("inbox")),
        message_id: Some(MessageId::from("message-1")),
        payload: json!({"messageId": "message-1"}),
    };
    let matching_filter = EventFilter {
        account_id: Some(AccountId::from("primary")),
        topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
        mailbox_id: Some(MailboxId::from("inbox")),
        after_seq: Some(4),
    };
    assert!(matches_event(&event, &matching_filter));
    assert!(matches_event(
        &event,
        &EventFilter {
            account_id: None,
            topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
            mailbox_id: Some(MailboxId::from("inbox")),
            after_seq: Some(4),
        }
    ));
    assert!(!matches_event(
        &event,
        &EventFilter {
            account_id: Some(AccountId::from("secondary")),
            topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
            mailbox_id: Some(MailboxId::from("inbox")),
            after_seq: Some(4),
        }
    ));
}

#[test]
fn api_error_maps_state_mismatch_to_conflict() {
    let error = ApiError::from_service_error(ServiceError::from(GatewayError::StateMismatch));

    assert_eq!(error.status, StatusCode::CONFLICT);
    assert_eq!(error.body.code, ApiErrorCode::StateMismatch);
}
