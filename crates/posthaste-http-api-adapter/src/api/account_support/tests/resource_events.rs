use super::*;
use crate::api::account_support::events::{ResourceChange, ResourceOperation};

#[test]
fn resource_change_serializes_wire_compatible_json() {
    let account_id = AccountId::from("primary");
    assert_eq!(
        serde_json::to_value(ResourceChange::account(
            ResourceOperation::Updated,
            &account_id,
        ))
        .expect("resource change should serialize"),
        json!({
            "kind": "account",
            "operation": "updated",
            "id": "primary",
            "accountId": "primary",
        }),
    );
}

#[test]
fn account_events_include_declarative_resource_payload() {
    let test = test_app_state();
    let account_id = AccountId::from("primary");
    append_and_publish_account_event(
        test.store.as_ref(),
        &test.event_sender,
        &account_id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    )
    .expect("account event should append");

    let events = test
        .service
        .list_events(&EventFilter {
            account_id: Some(account_id.clone()),
            topic: Some(EVENT_TOPIC_ACCOUNT_UPDATED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })
        .expect("events should list");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["accountId"], account_id.as_str());
    assert_eq!(events[0].payload["resources"][0]["kind"], "account");
    assert_eq!(events[0].payload["resources"][0]["operation"], "updated");
    assert_eq!(events[0].payload["resources"][0]["id"], account_id.as_str());
}
