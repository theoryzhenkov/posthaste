use posthaste_domain_model::{
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
    let mut changed = operation(
        "op-1",
        "draft-temp-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    );
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
fn applied_op_is_unsettled_for_overlay_but_excluded_from_pending() -> Result<(), StoreError> {
    // A flushed message assertion rests in `applied`: the read overlay folds it
    // (list_unsettled), but it is not shown as pending/UI work (list_pending),
    // until a sync retires it.
    //
    // @spec docs/replication/L1#retire-on-confirmation
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-1",
        "message-1",
        OperationKind::ReplaceMailboxes,
        OperationState::Applied,
    ))?;
    store.enqueue_operation(&operation(
        "op-2",
        "message-2",
        OperationKind::SetKeywords,
        OperationState::Pending,
    ))?;
    store.enqueue_operation(&operation(
        "op-3",
        "message-3",
        OperationKind::Destroy,
        OperationState::Failed,
    ))?;

    let unsettled = store.list_unsettled_operations(&account)?;
    let unsettled_ids: Vec<&str> = unsettled.iter().map(|op| op.id.as_str()).collect();
    assert_eq!(
        unsettled_ids,
        vec!["op-1", "op-2"],
        "overlay folds pending + applied"
    );

    let pending = store.list_pending_operations(&account)?;
    let pending_ids: Vec<&str> = pending.iter().map(|op| op.id.as_str()).collect();
    assert_eq!(
        pending_ids,
        vec!["op-2", "op-3"],
        "applied is excluded from pending"
    );

    // Retire the applied op (convergence) and it leaves both views.
    store.remove_operation(&OperationId::from("op-1"))?;
    assert!(store
        .list_unsettled_operations(&account)?
        .iter()
        .all(|op| op.id.as_str() != "op-1"));
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

#[test]
fn resolve_draft_entity_falls_back_to_projection_when_no_alias() -> Result<(), StoreError> {
    // The owner repro (DS2/D131): a draft synced from the server / created on
    // another device / surviving a restart has a `message` row keyed by its
    // live server Email id and carrying the stable `draft_id`, but NO
    // `draft_alias` (that is only populated by a create/save in THIS runtime).
    // The stable-id list-row discard must still resolve to the live Email id.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut draft = sample_message("E1", "inbox", Some("draft-mime"));
    draft.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![draft],
            ..SyncBatch::default()
        },
    )?;

    // No alias row exists, so resolution falls back to the projection and
    // returns the live/canonical server Email id.
    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        Some("E1".to_string())
    );
    Ok(())
}

#[test]
fn resolve_draft_entity_prefers_alias_over_projection() -> Result<(), StoreError> {
    // Precedence: an in-session `draft_alias` is the freshest create/rotate
    // mapping (possibly mid-id-rotation) and MUST win over the projection.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut draft = sample_message("E1", "inbox", Some("draft-mime"));
    draft.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![draft],
            ..SyncBatch::default()
        },
    )?;
    store.set_draft_alias(&account, "draft-local-X", "draft-temp-live")?;

    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        Some("draft-temp-live".to_string()),
        "the in-session alias wins over the projection row"
    );
    Ok(())
}

#[test]
fn resolve_draft_entity_none_when_absent_everywhere() -> Result<(), StoreError> {
    // A genuinely-absent draft (no alias, no projection row) still resolves to
    // None so the D133 NotFound guard fires and the client reverts.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-gone")?,
        None
    );
    Ok(())
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N15 / M27 sub-unit (b))
#[test]
fn flushable_operations_are_limited_and_drain_across_cycles() -> Result<(), StoreError> {
    // Stand-in for a large stuck outbox (N15): more pending ops than one
    // `list_flushable_operations` call should ever materialize at once.
    use crate::outbox::OUTBOX_FLUSH_BATCH_LIMIT;

    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let total = OUTBOX_FLUSH_BATCH_LIMIT as usize + 50;

    for index in 0..total {
        store.enqueue_operation(&operation(
            &format!("op-{index}"),
            &format!("draft-{index}"),
            OperationKind::DraftCreate,
            OperationState::Pending,
        ))?;
    }

    // First "cycle": the store returns at most the batch limit, not the
    // whole backlog in one unbounded `Vec`.
    let first_batch = store.list_flushable_operations(&account)?;
    assert_eq!(first_batch.len(), OUTBOX_FLUSH_BATCH_LIMIT as usize);

    // The flush loop processes a batch by moving each op out of the
    // flushable state set (here: straight to `applied`, standing in for a
    // successful push) before the next cycle's call — draining the backlog
    // across repeated bounded calls instead of requiring one giant read.
    for op in &first_batch {
        store.update_operation_state(&op.id, OperationState::Applied, 0, None)?;
    }

    let second_batch = store.list_flushable_operations(&account)?;
    assert_eq!(
        second_batch.len(),
        total - OUTBOX_FLUSH_BATCH_LIMIT as usize,
        "the remainder should surface on the next bounded call"
    );

    // No overlap between the two cycles' batches.
    let first_ids: std::collections::HashSet<&str> =
        first_batch.iter().map(|op| op.id.as_str()).collect();
    assert!(second_batch
        .iter()
        .all(|op| !first_ids.contains(op.id.as_str())));
    Ok(())
}
