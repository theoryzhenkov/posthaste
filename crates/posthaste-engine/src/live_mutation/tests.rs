use posthaste_domain_model::{GatewayError, MailboxId, MessageId, SetKeywordsCommand, SyncObject};
use serde_json::{json, Value};

use super::*;

#[test]
fn set_keywords_request_uses_null_to_remove_seen() {
    let request = set_keywords_request_body(
        "account-1",
        Some("state-1"),
        &MessageId::from("message-1"),
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: vec!["$seen".to_string()],
        },
    );

    assert_eq!(
        request["methodCalls"][0][1]["update"]["message-1"]["keywords/$flagged"],
        Value::Bool(true)
    );
    assert_eq!(
        request["methodCalls"][0][1]["update"]["message-1"]["keywords/$seen"],
        Value::Null
    );
    assert_eq!(request["methodCalls"][0][1]["ifInState"], "state-1");
}

#[test]
fn set_mailbox_role_request_uses_null_to_clear_role() {
    let request = set_mailbox_role_request_body(
        "account-1",
        Some("mailbox-state-1"),
        &MailboxId::from("mailbox-1"),
        None,
    );

    assert_eq!(
        request["methodCalls"][0][1]["update"]["mailbox-1"]["role"],
        Value::Null
    );
    assert_eq!(request["methodCalls"][0][1]["ifInState"], "mailbox-state-1");
}

#[test]
fn set_mailbox_role_request_sets_archive_role() {
    let request = set_mailbox_role_request_body(
        "account-1",
        None,
        &MailboxId::from("mailbox-1"),
        Some("archive"),
    );

    assert_eq!(
        request["methodCalls"][0][1]["update"]["mailbox-1"]["role"],
        Value::String("archive".to_string())
    );
    assert!(request["methodCalls"][0][1].get("ifInState").is_none());
}

#[test]
fn set_mailbox_role_request_clears_role() {
    let request = set_mailbox_role_request_body(
        "account-1",
        Some("mailbox-state-1"),
        &MailboxId::from("archive-owner"),
        None,
    );

    assert_eq!(
        request["methodCalls"][0][1]["update"]["archive-owner"]["role"],
        Value::Null
    );
}

#[test]
fn create_mailbox_request_builds_flat_create_with_name_and_no_parent() {
    let request = create_mailbox_request_body("account-1", "Receipts");

    let create = &request["methodCalls"][0][1]["create"][CREATE_MAILBOX_CREATE_ID];
    assert_eq!(create["name"], Value::String("Receipts".to_string()));
    // Flat create: no parentId is sent.
    assert!(
        create.get("parentId").is_none(),
        "a flat create must not carry a parentId"
    );
    assert_eq!(request["methodCalls"][0][0], "Mailbox/set");
    // A create carries no `ifInState`/`update` — those are the role-patch path.
    assert!(request["methodCalls"][0][1].get("update").is_none());
}

#[test]
fn created_mailbox_id_parses_server_id_from_created_map() {
    let response: jmap_client::core::response::MailboxSetResponse = serde_json::from_value(json!({
        "accountId": "account-1",
        "oldState": "mailbox-1",
        "newState": "mailbox-2",
        "created": {
            CREATE_MAILBOX_CREATE_ID: { "id": "MB123", "name": "Receipts" }
        }
    }))
    .expect("create response should deserialize");

    let id = created_mailbox_id(response, CREATE_MAILBOX_CREATE_ID)
        .expect("the created server id should parse");
    assert_eq!(id.as_str(), "MB123");
}

#[test]
fn set_keywords_outcome_requires_target_id_to_be_updated() {
    let response: jmap_client::core::response::EmailSetResponse = serde_json::from_value(json!({
        "accountId": "account-1",
        "oldState": "state-1",
        "newState": "state-2",
        "notUpdated": {
            "message-1": {
                "type": "invalidProperties",
                "description": "bad keyword patch"
            }
        }
    }))
    .expect("set response should deserialize");

    let error = set_keywords_mutation_outcome(response, &MessageId::from("message-1"))
        .expect_err("notUpdated must not be treated as success");

    match error {
        GatewayError::Rejected(message) => {
            assert!(message.contains("bad keyword patch"));
        }
        other => panic!("expected rejected error, got {other:?}"),
    }
}

#[test]
fn mailbox_mutation_outcome_wraps_mailbox_cursor() {
    let response: jmap_client::core::response::MailboxSetResponse = serde_json::from_value(json!({
        "accountId": "account-1",
        "oldState": "mailbox-1",
        "newState": "mailbox-2",
        "updated": {
            "archive": null
        }
    }))
    .expect("set response should deserialize");

    let outcome = mailbox_mutation_outcome(response, &MailboxId::from("archive"))
        .expect("cursor should build");
    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Mailbox);
    assert_eq!(cursor.state, "mailbox-2");
}

#[test]
fn mailbox_mutation_outcome_wraps_target_cursor_when_other_ids_updated() {
    let response: jmap_client::core::response::MailboxSetResponse = serde_json::from_value(json!({
        "accountId": "account-1",
        "oldState": "mailbox-1",
        "newState": "mailbox-2",
        "updated": {
            "archive-target": null,
            "archive-owner": null
        }
    }))
    .expect("set response should deserialize");

    let outcome = mailbox_mutation_outcome(response, &MailboxId::from("archive-target"))
        .expect("cursor should build");
    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Mailbox);
    assert_eq!(cursor.state, "mailbox-2");
}

#[test]
fn message_mutation_outcome_wraps_message_cursor() {
    let outcome = message_mutation_outcome("message-9".to_string()).expect("cursor should build");
    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Message);
    assert_eq!(
        crate::sync::decode_email_cursor_state(&cursor.state),
        Some("message-9".to_string())
    );
    assert!(!cursor.updated_at.is_empty());
}
