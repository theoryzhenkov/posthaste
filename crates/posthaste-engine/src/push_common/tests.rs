use posthaste_domain_service::AccountId;
use serde_json::json;

use super::{convert_sse_push_notification, convert_ws_push_object};

#[test]
fn ws_push_filters_by_server_account_and_emits_local_account() {
    let push = serde_json::from_value(json!({
        "@type": "StateChange",
        "changed": {
            "server-account": {
                "Email": "state-1",
                "Mailbox": "state-2"
            }
        }
    }))
    .expect("push object");

    let notification =
        convert_ws_push_object(&AccountId::from("local-account"), "server-account", push)
            .expect("conversion")
            .expect("notification");

    assert_eq!(notification.account_id, AccountId::from("local-account"));
    assert_eq!(notification.checkpoint, None);
    assert_eq!(notification.changed.len(), 2);
    assert!(notification.changed.contains(&"Email".to_string()));
    assert!(notification.changed.contains(&"Mailbox".to_string()));
}

#[test]
fn ws_push_ignores_other_server_accounts() {
    let push = serde_json::from_value(json!({
        "@type": "StateChange",
        "changed": {
            "other-server-account": {
                "Email": "state-1"
            }
        }
    }))
    .expect("push object");

    let notification =
        convert_ws_push_object(&AccountId::from("local-account"), "server-account", push)
            .expect("conversion");

    assert!(notification.is_none());
}

#[test]
fn sse_push_filters_by_server_account_and_preserves_checkpoint() {
    let push = jmap_client::event_source::PushNotification::StateChange(
        serde_json::from_value(changes()).expect("changes"),
    );

    let notification =
        convert_sse_push_notification(&AccountId::from("local-account"), "server-account", push)
            .expect("conversion")
            .expect("notification");

    assert_eq!(notification.account_id, AccountId::from("local-account"));
    assert_eq!(notification.checkpoint, Some("event-42".to_string()));
    assert_eq!(notification.changed.len(), 2);
    assert!(notification.changed.contains(&"Email".to_string()));
    assert!(notification.changed.contains(&"Mailbox".to_string()));
}

#[test]
fn sse_push_ignores_other_server_accounts() {
    let push = jmap_client::event_source::PushNotification::StateChange(
        serde_json::from_value(changes()).expect("changes"),
    );

    let notification =
        convert_sse_push_notification(&AccountId::from("local-account"), "missing-account", push)
            .expect("conversion");

    let notification = notification.expect("checkpoint-only notification");
    assert_eq!(notification.account_id, AccountId::from("local-account"));
    assert_eq!(notification.checkpoint, Some("event-42".to_string()));
    assert!(notification.changed.is_empty());
}

fn changes() -> serde_json::Value {
    json!({
        "id": "event-42",
        "changes": {
            "server-account": {
                "Email": "state-1",
                "Mailbox": "state-2"
            }
        }
    })
}
