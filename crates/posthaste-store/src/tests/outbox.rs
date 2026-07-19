use posthaste_domain_model::{
    Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind, OperationState,
};
use serde_json::json;

use super::*;

/// The flush gate's "now" for tests: ops without `send_at` are due at any
/// time, so any canonical instant works where no schedule is involved.
const NOW: &str = "2026-06-21T00:00:00Z";

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
        payload_version: 1,
        state,
        attempts: 0,
        last_error: None,
        send_at: None,
        hold_until_mono: None,
        created_at: NOW.to_string(),
        updated_at: NOW.to_string(),
    }
}

fn scheduled_send(id: &str, send_at: &str, state: OperationState) -> Operation {
    let mut op = operation(id, &format!("send-{id}"), OperationKind::Send, state);
    op.send_at = Some(send_at.to_string());
    op
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

    let flushable = store.list_flushable_operations(&account, NOW, 0)?;
    let ids: Vec<&str> = flushable.iter().map(|op| op.id.as_str()).collect();
    // op-2 is applied and op-4 is failed, so both are excluded; order follows insertion.
    assert_eq!(ids, vec!["op-1", "op-3"]);
    Ok(())
}

#[test]
fn applied_op_is_unsettled_for_overlay_but_excluded_from_pending() -> Result<(), StoreError> {
    // A flushed message assertion rests in `applied`: the read overlay folds it
    // (list_unsettled), but it is not shown as pending/UI work (list_pending),
    // until causal truncation removes it.
    //
    // @spec docs/backend/L2-optimism#settlement-and-truncation
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
    let first_batch = store.list_flushable_operations(&account, NOW, 0)?;
    assert_eq!(first_batch.len(), OUTBOX_FLUSH_BATCH_LIMIT as usize);

    // The flush loop processes a batch by moving each op out of the
    // flushable state set (here: straight to `applied`, standing in for a
    // successful push) before the next cycle's call — draining the backlog
    // across repeated bounded calls instead of requiring one giant read.
    for op in &first_batch {
        store.update_operation_state(&op.id, OperationState::Applied, 0, None)?;
    }

    let second_batch = store.list_flushable_operations(&account, NOW, 0)?;
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

// ---------------------------------------------------------------------------
// Scheduled sends (undo-send / send-later): the `send_at` hold + the atomic
// cancel-vs-flush primitives. @spec docs/L1-outbox#operation-model
// ---------------------------------------------------------------------------

#[test]
fn scheduled_send_is_held_until_due_and_survives_reopen() -> Result<(), StoreError> {
    let root = temp_root();
    let account = AccountId::from("primary");
    {
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        store.enqueue_operation(&scheduled_send(
            "op-later",
            "2026-06-21T00:00:10Z",
            OperationState::Pending,
        ))?;
        // Before due: held out of the flushable set (but still pending/visible).
        assert!(store
            .list_flushable_operations(&account, "2026-06-21T00:00:09Z", 0)?
            .is_empty());
        assert_eq!(store.list_pending_operations(&account)?.len(), 1);
    }
    // Simulated restart: a fresh store over the same database still holds the
    // schedule (it is a persisted column, not process state) and releases it
    // exactly at the boundary (`send_at <= now` — due AT the instant).
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    assert!(store
        .list_flushable_operations(&account, "2026-06-21T00:00:09Z", 0)?
        .is_empty());
    let due = store.list_flushable_operations(&account, "2026-06-21T00:00:10Z", 0)?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id.as_str(), "op-later");
    assert_eq!(due[0].send_at.as_deref(), Some("2026-06-21T00:00:10Z"));
    Ok(())
}

#[test]
fn count_due_scheduled_sends_counts_only_due_queued_schedules() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&scheduled_send(
        "op-due",
        "2026-06-21T00:00:00Z",
        OperationState::Pending,
    ))?;
    store.enqueue_operation(&scheduled_send(
        "op-future",
        "2026-06-21T01:00:00Z",
        OperationState::Pending,
    ))?;
    // A parked (dispatch-uncertain) send is not auto-flushable and must not
    // re-trigger the scheduler tick either.
    store.enqueue_operation(&scheduled_send(
        "op-parked",
        "2026-06-21T00:00:00Z",
        OperationState::DispatchUncertain,
    ))?;
    // An unscheduled op never counts.
    store.enqueue_operation(&operation(
        "op-plain",
        "draft-1",
        OperationKind::DraftCreate,
        OperationState::Pending,
    ))?;

    assert_eq!(store.count_due_scheduled_sends(&account, NOW, 0)?, 1);
    assert_eq!(
        store.count_due_scheduled_sends(&account, "2026-06-21T01:00:00Z", 0)?,
        2
    );
    assert_eq!(
        store.count_due_scheduled_sends(&account, "2026-06-20T23:59:59Z", 0)?,
        0
    );
    Ok(())
}

#[test]
fn claim_then_cancel_has_exactly_one_winner() -> Result<(), StoreError> {
    // Flush wins: after the guarded claim, the guarded cancel removes nothing.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let op = scheduled_send("op-race", "2026-06-21T00:00:00Z", OperationState::Pending);
    store.enqueue_operation(&op)?;

    assert!(
        store.claim_operation_for_flush(&op.id)?,
        "flush claims first"
    );
    assert!(
        !store.remove_operation_unless_inflight(&op.id)?,
        "a claimed (inflight) op must not be cancelable"
    );
    let stored = store.get_operation(&op.id)?.expect("still queued");
    assert_eq!(stored.state, OperationState::Inflight);
    Ok(())
}

#[test]
fn cancel_then_claim_has_exactly_one_winner() -> Result<(), StoreError> {
    // Cancel wins: after the guarded removal, the guarded claim matches
    // nothing, so the flusher skips — the canceled send is never pushed.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let op = scheduled_send("op-race", "2026-06-21T00:00:00Z", OperationState::Pending);
    store.enqueue_operation(&op)?;

    assert!(
        store.remove_operation_unless_inflight(&op.id)?,
        "cancel wins"
    );
    assert!(
        !store.claim_operation_for_flush(&op.id)?,
        "a canceled op must not be claimable for flush"
    );
    assert!(store.get_operation(&op.id)?.is_none());
    Ok(())
}

#[test]
fn cancel_of_a_failed_or_missing_op_keeps_prior_semantics() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    // Missing: nothing to remove.
    assert!(!store.remove_operation_unless_inflight(&OperationId::from("op-gone"))?);
    // Failed: still discardable (the pre-existing dead-op escape hatch).
    let op = operation(
        "op-failed",
        "draft-1",
        OperationKind::DraftCreate,
        OperationState::Failed,
    );
    store.enqueue_operation(&op)?;
    assert!(store.remove_operation_unless_inflight(&op.id)?);
    Ok(())
}

#[test]
fn cancel_of_a_settled_op_leaves_it_in_the_log() -> Result<(), StoreError> {
    // A settled (`applied`) op rests in the log until causal truncation: the
    // provider already accepted its mutation, so a late cancel must not
    // delete it (that would drop its replay fold before base catches up).
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let op = operation(
        "op-settled",
        "message-1",
        OperationKind::SetKeywords,
        OperationState::Inflight,
    );
    store.enqueue_operation(&op)?;
    store.mark_operation_settled(&op.id, 1_234, Some("state-w"))?;

    assert!(
        !store.remove_operation_unless_inflight(&op.id)?,
        "a settled op must not be cancelable"
    );
    assert_eq!(
        store.list_unsettled_operations(&account)?.len(),
        1,
        "the settled op still folds in the replay read"
    );
    Ok(())
}

#[test]
fn mark_operation_settled_persists_markers_and_filters() -> Result<(), StoreError> {
    // Settling IN PLACE: state becomes 'applied' with the causal-truncation
    // markers persisted; the op leaves the flush lane and the pending (UI)
    // list but stays in the unsettled (replay) list until truncation removes
    // it.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-1",
        "message-1",
        OperationKind::SetKeywords,
        OperationState::Inflight,
    ))?;
    store.mark_operation_settled(&OperationId::from("op-1"), 1_234, Some("state-w"))?;

    let settled = store.list_settled_operations(&account)?;
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].operation.id.as_str(), "op-1");
    assert_eq!(settled[0].operation.state, OperationState::Applied);
    assert_eq!(settled[0].settled_at_mono, Some(1_234));
    assert_eq!(settled[0].watermark.as_deref(), Some("state-w"));

    assert!(
        store
            .list_flushable_operations(&account, NOW, 0)?
            .is_empty(),
        "a settled op is out of the flush lane — never re-delivered"
    );
    assert!(
        store.list_pending_operations(&account)?.is_empty(),
        "a settled op is not user-facing outstanding work"
    );
    assert_eq!(
        store.list_unsettled_operations(&account)?.len(),
        1,
        "a settled op still folds in the replay read"
    );

    store.remove_operation(&OperationId::from("op-1"))?;
    assert!(store.list_settled_operations(&account)?.is_empty());
    Ok(())
}

#[test]
fn settled_list_orders_by_insertion_and_scopes_by_account() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-1",
        "message-1",
        OperationKind::SetKeywords,
        OperationState::Inflight,
    ))?;
    store.enqueue_operation(&operation(
        "op-2",
        "message-2",
        OperationKind::Destroy,
        OperationState::Inflight,
    ))?;
    let mut other = operation(
        "op-other",
        "message-3",
        OperationKind::SetKeywords,
        OperationState::Inflight,
    );
    other.account_id = AccountId::from("secondary");
    store.enqueue_operation(&other)?;

    store.mark_operation_settled(&OperationId::from("op-2"), 20, None)?;
    store.mark_operation_settled(&OperationId::from("op-1"), 10, Some("w-1"))?;
    store.mark_operation_settled(&OperationId::from("op-other"), 30, None)?;

    let settled = store.list_settled_operations(&account)?;
    let ids: Vec<&str> = settled
        .iter()
        .map(|settled| settled.operation.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["op-1", "op-2"],
        "insertion order, own account only"
    );
    assert_eq!(settled[0].watermark.as_deref(), Some("w-1"));
    assert_eq!(settled[1].watermark, None, "no usable provider position");
    Ok(())
}

#[test]
fn legacy_applied_row_reads_with_null_markers() -> Result<(), StoreError> {
    // A row settled before the marker columns existed (or written 'applied'
    // directly) carries NULL markers: it must read back as `None`/`None` —
    // the truncation pass treats it as eligible on any completed cycle.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    store.enqueue_operation(&operation(
        "op-legacy",
        "message-1",
        OperationKind::SetKeywords,
        OperationState::Applied,
    ))?;

    let settled = store.list_settled_operations(&account)?;
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].settled_at_mono, None);
    assert_eq!(settled[0].watermark, None);
    Ok(())
}

#[test]
fn settled_marker_columns_are_idempotent_across_reopen() -> Result<(), StoreError> {
    // The additive `ensure_column` evolution must be idempotent: reopening an
    // existing database re-runs init_schema over the already-added columns,
    // and persisted markers survive the reopen.
    let root = temp_root();
    let path = root.join("mail.sqlite");
    {
        let store = DatabaseStore::open(path.clone(), root.join("data"))?;
        store.enqueue_operation(&operation(
            "op-1",
            "message-1",
            OperationKind::SetKeywords,
            OperationState::Inflight,
        ))?;
        store.mark_operation_settled(&OperationId::from("op-1"), 77, Some("w-77"))?;
    }
    let reopened = DatabaseStore::open(path, root.join("data"))?;
    let settled = reopened.list_settled_operations(&AccountId::from("primary"))?;
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].settled_at_mono, Some(77));
    assert_eq!(settled[0].watermark.as_deref(), Some("w-77"));
    Ok(())
}
