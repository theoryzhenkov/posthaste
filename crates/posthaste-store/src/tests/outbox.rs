use posthaste_domain::{
    Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind, OperationState,
};
use serde_json::json;

use super::*;

fn operation(id: &str, entity_id: &str, kind: OperationKind, state: OperationState) -> Operation {
    Operation {
        id: OperationId::from(id),
        account_id: AccountId::from("primary"),
        entity: OperationEntity {
            kind: OperationEntityKind::Draft,
            id: entity_id.to_string(),
        },
        kind,
        payload: json!({ "subject": "Hi" }),
        state,
        attempts: 0,
        last_error: None,
        depends_on: None,
        created_at: "2026-06-21T00:00:00Z".to_string(),
        updated_at: "2026-06-21T00:00:00Z".to_string(),
    }
}

#[test]
fn enqueue_is_idempotent_on_operation_id() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    let op = operation(
        "op-1",
        "draft-temp-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    );
    store.enqueue_operation(&op)?;

    // Re-enqueuing the same id is a no-op and does not duplicate or overwrite.
    let mut changed = op.clone();
    changed.payload = json!({ "subject": "Changed" });
    let stored = store.enqueue_operation(&changed)?;
    assert_eq!(stored.payload, json!({ "subject": "Hi" }));

    let pending = store.list_pending_operations(&account)?;
    assert_eq!(pending.len(), 1);
    Ok(())
}

#[test]
fn flushable_lists_pending_and_inflight_in_insertion_order() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-1",
        "draft-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    ))?;
    store.enqueue_operation(&operation(
        "op-2",
        "draft-1",
        OperationKind::DraftUpdate,
        OperationState::Applied,
    ))?;
    store.enqueue_operation(&operation(
        "op-3",
        "draft-2",
        OperationKind::DraftCreate,
        OperationState::Inflight,
    ))?;
    store.enqueue_operation(&operation(
        "op-4",
        "draft-3",
        OperationKind::DraftCreate,
        OperationState::Failed,
    ))?;

    let flushable = store.list_flushable_operations(&account)?;
    let ids: Vec<&str> = flushable.iter().map(|op| op.id.as_str()).collect();
    // op-2 is applied and op-4 is failed, so both are excluded; order follows insertion.
    assert_eq!(ids, vec!["op-1", "op-3"]);
    Ok(())
}

#[test]
fn update_state_records_attempts_and_error() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;

    let op = operation(
        "op-1",
        "draft-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    );
    store.enqueue_operation(&op)?;

    store.update_operation_state(&op.id, OperationState::Failed, 3, Some("boom"))?;

    let stored = store.get_operation(&op.id)?.expect("operation present");
    assert_eq!(stored.state, OperationState::Failed);
    assert_eq!(stored.attempts, 3);
    assert_eq!(stored.last_error.as_deref(), Some("boom"));
    Ok(())
}

#[test]
fn reconcile_entity_id_rewrites_all_account_operations() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-1",
        "draft-temp",
        OperationKind::DraftCreate,
        OperationState::Pending,
    ))?;
    store.enqueue_operation(&operation(
        "op-2",
        "draft-temp",
        OperationKind::DraftUpdate,
        OperationState::Pending,
    ))?;

    store.reconcile_operation_entity_id(&account, "draft-temp", "provider-id-7")?;

    let pending = store.list_pending_operations(&account)?;
    assert!(pending.iter().all(|op| op.entity.id == "provider-id-7"));
    Ok(())
}

#[test]
fn remove_deletes_the_operation() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;

    let op = operation(
        "op-1",
        "draft-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    );
    store.enqueue_operation(&op)?;
    store.remove_operation(&op.id)?;

    assert!(store.get_operation(&op.id)?.is_none());
    Ok(())
}
