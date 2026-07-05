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
fn sync_write_through_registers_a_synced_draft_for_registry_only_resolution(
) -> Result<(), StoreError> {
    // The owner repro (DS2/D131), M69 shape: a draft synced from the server /
    // created on another device / surviving a restart has a `message` row keyed
    // by its live server Email id and carrying the stable `draft_id`, with no
    // prior in-session registry row. Sync's in-transaction write-through (D135)
    // registers stable key → live Email id, so the stable-id list-row discard
    // resolves to the live id via the registry ALONE — the projection fallback
    // is deleted.
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

    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        Some("E1".to_string()),
        "sync write-through must register the synced draft in the registry"
    );

    // Prove resolution consults the registry ALONE: with the registry row
    // removed, the surviving `message.draft_id` projection row must NOT be
    // consulted (the D131 alias-then-projection fallback is gone).
    store.remove_draft_alias(&account, "draft-local-X")?;
    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        None,
        "no projection fallback: resolution is one SELECT against the registry"
    );
    Ok(())
}

#[test]
fn in_session_save_and_sync_write_the_same_registry() -> Result<(), StoreError> {
    // In-session saves and sync now share ONE table (M69): a later in-session
    // save (possibly mid-id-rotation, mapping the key to a temp entity id)
    // overwrites the sync-written row, and resolution returns the latest write.
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
        "the in-session save is the latest registry write and wins"
    );
    Ok(())
}

#[test]
fn sync_observed_rotation_repoints_the_registry() -> Result<(), StoreError> {
    // Rotation observed by sync (another device / a past session saved the
    // draft, rotating its provider id E1 → E2): the write-through repoints the
    // registry to the new live id — in the same batch (delete + upsert) and
    // across batches (upsert first, stale-row delete later).
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut draft_v1 = sample_message("E1", "inbox", Some("draft-mime"));
    draft_v1.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![draft_v1],
            ..SyncBatch::default()
        },
    )?;

    // Same batch: sync reports E1 deleted and delivers the successor E2.
    let mut draft_v2 = sample_message("E2", "inbox", Some("draft-mime-2"));
    draft_v2.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            deleted_message_ids: vec![MessageId::from("E1")],
            messages: vec![draft_v2],
            ..SyncBatch::default()
        },
    )?;
    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        Some("E2".to_string()),
        "an observed rotation repoints the registry to the new live id"
    );

    // Across batches: the successor E3 arrives first; the stale E2 delete in a
    // later batch must not clobber the already-repointed mapping.
    let mut draft_v3 = sample_message("E3", "inbox", Some("draft-mime-3"));
    draft_v3.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![draft_v3],
            ..SyncBatch::default()
        },
    )?;
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            deleted_message_ids: vec![MessageId::from("E2")],
            ..SyncBatch::default()
        },
    )?;
    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        Some("E3".to_string()),
        "a stale-row delete after the successor synced leaves the mapping fresh"
    );
    Ok(())
}

#[test]
fn sync_confirmed_gone_forgets_the_registry_mapping() -> Result<(), StoreError> {
    // Confirmed-gone: sync deletes the draft row and no projected row carries
    // the key anymore — the registry forgets, so the key resolves to None and
    // the D133 NotFound guard fires on a subsequent discard.
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
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            deleted_message_ids: vec![MessageId::from("E1")],
            ..SyncBatch::default()
        },
    )?;

    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        None,
        "a sync-confirmed deletion forgets the registry mapping"
    );
    Ok(())
}

#[test]
fn snapshot_prune_forgets_a_pruned_drafts_registry_mapping() -> Result<(), StoreError> {
    // The prune-by-absence path (replace_all_messages snapshot) is a sync
    // deletion too: a draft absent from the authoritative remote set is pruned
    // AND its registry mapping is forgotten in the same transaction.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut draft = sample_message("E1", "inbox", Some("draft-mime"));
    draft.draft_id = Some("draft-local-X".to_string());
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            messages: vec![
                draft,
                sample_message("M2", "inbox", Some("mime-2")),
                sample_message("M3", "inbox", Some("mime-3")),
            ],
            ..SyncBatch::default()
        },
    )?;

    // Full snapshot without the draft: E1 is pruned (1 of 3 locals — under the
    // DS1 floor), M2/M3 survive.
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            replace_all_messages: true,
            messages: vec![
                sample_message("M2", "inbox", Some("mime-2")),
                sample_message("M3", "inbox", Some("mime-3")),
            ],
            ..SyncBatch::default()
        },
    )?;

    assert_eq!(
        store.resolve_draft_entity(&account, "draft-local-X")?,
        None,
        "a snapshot prune of the draft row forgets its registry mapping"
    );
    Ok(())
}

#[test]
fn resolve_draft_entity_none_when_absent() -> Result<(), StoreError> {
    // A genuinely-absent draft (no registry row) resolves to None so the D133
    // NotFound guard fires and the client reverts.
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
