use super::*;
use crate::sql_cache::CachedSql;

/// DS1 mail-loss floor guard threshold. A full-snapshot prune-by-absence that
/// would delete more than this fraction of the local message store is refused
/// (unless an explicit full-resync signal is set), so a transiently-empty or
/// still-incomplete remote id set cannot silently wipe local mail. Ordinary
/// deletions (a few messages gone from an otherwise-matching remote set) fall
/// far below this and prune normally.
const MAX_ABSENCE_PRUNE_FRACTION: f64 = 0.5;

pub(crate) fn stage_sync_bodies(
    store: &DatabaseStore,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged: &mut StagedBodyFiles,
) -> Result<Vec<Option<RawMessageRef>>, StoreError> {
    batch
        .messages
        .iter()
        .map(|message| {
            let raw_mime = message
                .raw_mime
                .clone()
                .or_else(|| synthesize_raw_mime(message));
            raw_mime
                .as_deref()
                .map(|raw_mime| store.store_raw_message(account_id, raw_mime, staged))
                .transpose()
        })
        .collect()
}

/// Empty protected set shared by the unprotected `apply_sync_batch_tx`/
/// `reconcile_sync_tx` entry points, so their prune passes behave exactly as
/// before the S3 replace_all guard was added.
fn no_protected_message_ids() -> HashSet<String> {
    HashSet::new()
}

pub(crate) fn apply_sync_batch_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged_bodies: &[Option<RawMessageRef>],
) -> Result<Vec<DomainEvent>, StoreError> {
    apply_sync_batch_tx_impl(
        tx,
        account_id,
        batch,
        staged_bodies,
        &no_protected_message_ids(),
    )
}

/// Like [`apply_sync_batch_tx`], but for a `replace_all_messages` snapshot:
/// `protected_message_ids` (messages with an un-acked optimistic op) are
/// excluded from the prune-by-absence pass (M35 durable snapshot guard, D93).
/// The caller folds present rows in-batch and only omits absent/folded-to-
/// removed ones; this exemption keeps the prune from treating those omissions
/// as "deleted remotely".
pub(crate) fn apply_sync_batch_protected_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged_bodies: &[Option<RawMessageRef>],
    protected_message_ids: &HashSet<String>,
) -> Result<Vec<DomainEvent>, StoreError> {
    apply_sync_batch_tx_impl(tx, account_id, batch, staged_bodies, protected_message_ids)
}

fn apply_sync_batch_tx_impl(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged_bodies: &[Option<RawMessageRef>],
    protected_message_ids: &HashSet<String>,
) -> Result<Vec<DomainEvent>, StoreError> {
    let mut events =
        EventRecorder::with_capacity(tx, account_id, estimate_sync_event_count(batch))?;
    let mut affected = ProjectionInputs::default();

    if batch.replace_all_mailboxes {
        let remote_mailbox_ids: BTreeSet<_> = batch
            .mailboxes
            .iter()
            .map(|mailbox| mailbox.id.clone())
            .collect();
        prune_mailboxes_absent_from_remote_tx(tx, account_id, &remote_mailbox_ids, &mut events)?;
    }

    if batch.replace_all_messages {
        let remote_message_ids: BTreeSet<_> = batch
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect();
        prune_messages_absent_from_remote_tx(
            tx,
            account_id,
            &remote_message_ids,
            protected_message_ids,
            // No wired explicit-full-resync signal today; the floor guard is
            // always active on this path (DS1).
            false,
            &mut affected,
            &mut events,
        )?;

        prune_stale_imap_message_locations_for_snapshot_tx(
            tx,
            account_id,
            &batch.imap_message_locations,
        )?;
    }

    for mailbox_id in &batch.deleted_mailbox_ids {
        delete_mailbox_and_track_projection_inputs(tx, account_id, mailbox_id, &mut events)?;
        events.record(
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
        )?;
    }

    let deleted_message_ids = batch
        .deleted_message_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for location in &batch.deleted_imap_message_locations {
        if deleted_message_ids.contains(&location.message_id) {
            continue;
        }
        delete_imap_message_location_and_track_projection_inputs(
            tx,
            account_id,
            location,
            &mut events,
        )?;
    }

    for message_id in &batch.deleted_message_ids {
        // Capture the message's mailboxes before the delete so we can report the
        // (now decremented) counts; the store handles the row via `deleted:true`
        // but needs countDeltas to keep the sidebar live (was projection-less).
        let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
        delete_message_and_track_projection_inputs(tx, account_id, message_id, &mut affected)?;
        write_through_draft_registry_on_message_delete_tx(tx, account_id, message_id)?;
        let count_deltas =
            crate::query::mailbox_counts_json_tx(tx, account_id, previous_mailboxes.iter())?;
        let mut payload = json!({ "messageId": message_id.as_str(), "deleted": true });
        payload["countDeltas"] = count_deltas;
        events.record(
            EVENT_TOPIC_MESSAGE_UPDATED,
            previous_mailboxes.first(),
            Some(message_id),
            payload,
        )?;
    }

    for mailbox in &batch.mailboxes {
        let effective_role =
            effective_mailbox_role_tx(tx, account_id, &mailbox.id, mailbox.role.as_deref())?;
        tx.execute_cached(
            "INSERT INTO mailbox (account_id, id, name, role)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, id) DO UPDATE SET
                name = excluded.name,
                role = excluded.role",
            params![
                account_id.as_str(),
                mailbox.id.as_str(),
                mailbox.name,
                effective_role,
            ],
        )
        .map_err(sql_to_store_error)?;
        events.record(
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(&mailbox.id),
            None,
            json!({ "mailboxId": mailbox.id.as_str() }),
        )?;
    }

    for (message, raw_ref) in batch.messages.iter().zip(staged_bodies.iter()) {
        apply_message_record_tx(
            tx,
            account_id,
            message,
            raw_ref.as_ref(),
            &mut affected,
            &mut events,
        )?;
        write_through_draft_registry_on_message_upsert_tx(tx, account_id, message)?;
    }

    // Pure single-statement loops over the whole batch: hoist one prepared
    // statement and reuse it, the canonical rusqlite bulk-write idiom. (The
    // per-message helpers stay on `prepare_cached` since they are called once
    // per message with tiny inner loops.)
    {
        let mut insert_location = tx
            .prepare(
                "INSERT INTO imap_message_location (
                    account_id, message_id, mailbox_id, uid_validity, uid, modseq, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, message_id, mailbox_id) DO UPDATE SET
                    uid_validity = excluded.uid_validity,
                    uid = excluded.uid,
                    modseq = excluded.modseq,
                    updated_at = excluded.updated_at",
            )
            .map_err(sql_to_store_error)?;
        for location in &batch.imap_message_locations {
            insert_location
                .execute(params![
                    account_id.as_str(),
                    location.message_id.as_str(),
                    location.mailbox_id.as_str(),
                    location.uid_validity.0,
                    location.uid.0,
                    location.modseq.map(|modseq| modseq.0.to_string()),
                    location.updated_at,
                ])
                .map_err(sql_to_store_error)?;
        }
    }

    {
        let mut insert_state = tx
            .prepare(
                "INSERT INTO imap_mailbox_sync_state (
                    account_id, mailbox_id, mailbox_name, uid_validity,
                    highest_uid, highest_modseq, partial_initial_uid, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(account_id, mailbox_id) DO UPDATE SET
                    mailbox_name = excluded.mailbox_name,
                    uid_validity = excluded.uid_validity,
                    highest_uid = excluded.highest_uid,
                    highest_modseq = excluded.highest_modseq,
                    partial_initial_uid = excluded.partial_initial_uid,
                    updated_at = excluded.updated_at",
            )
            .map_err(sql_to_store_error)?;
        for state in &batch.imap_mailbox_states {
            insert_state
                .execute(params![
                    account_id.as_str(),
                    state.mailbox_id.as_str(),
                    state.mailbox_name,
                    state.uid_validity.0,
                    state.highest_uid.map(|uid| uid.0),
                    state.highest_modseq.map(|modseq| modseq.0.to_string()),
                    state.partial_initial_uid.map(|uid| uid.0),
                    state.updated_at,
                ])
                .map_err(sql_to_store_error)?;
        }
    }

    for thread_id in affected.threads {
        refresh_thread_projection_tx(tx, account_id, &thread_id)?;
    }
    for conversation_id in affected.conversations {
        refresh_conversation_projection_tx(tx, &conversation_id)?;
    }
    for cursor in &batch.cursors {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }

    Ok(events.into_events())
}

/// Delete every local mailbox whose id is absent from the complete remote set,
/// recording a `deleted` mailbox event for each. Shared by the in-batch
/// `replace_all_mailboxes` snapshot path and the streamed final-reconciliation
/// pass, which both prune by difference against the authoritative remote ids.
///
pub(crate) fn prune_mailboxes_absent_from_remote_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    remote_mailbox_ids: &BTreeSet<MailboxId>,
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare("SELECT id FROM mailbox WHERE account_id = ?1")
        .map_err(sql_to_store_error)?;
    let local_mailbox_ids = statement
        .query_map(params![account_id.as_str()], |row| row.get::<_, String>(0))
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?
        .into_iter()
        .map(MailboxId)
        .collect::<BTreeSet<_>>();

    for mailbox_id in local_mailbox_ids.difference(remote_mailbox_ids) {
        delete_mailbox_and_track_projection_inputs(tx, account_id, mailbox_id, events)?;
        events.record(
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
        )?;
    }
    Ok(())
}

/// Delete every local message whose id is absent from the complete remote set,
/// recording a `deleted` message event for each. Shared by the in-batch
/// `replace_all_messages` snapshot path and the streamed final-reconciliation
/// pass.
///
/// `protected_message_ids` (messages with an un-acked optimistic op, M35
/// durable snapshot guard) are skipped even when absent from `remote_message_ids`:
/// a not-yet-uploaded local message, or one the guard left out of the snapshot
/// because it folded to removed (pending Destroy), must survive this pass and
/// reconcile once its op settles. Rows the guard *did* fold into the snapshot
/// are present in `remote_message_ids`, so they are not pruned regardless.
///
/// DS1 mail-loss floor guard: unless `force_full_prune` is set (an explicit
/// full-resync signal), this refuses to run when `remote_message_ids` is empty
/// or would delete more than [`MAX_ABSENCE_PRUNE_FRACTION`] of the local store.
/// A transiently-empty-but-`Ok` remote query, or a still-incomplete remote id
/// set that slipped past the caller's completeness check, must never silently
/// wipe local mail. A single ordinary deletion (remote only slightly smaller
/// than local) stays well under the floor and prunes normally; a legitimate
/// mass deletion must arrive through `force_full_prune`, not an unbounded
/// absence-prune.
pub(crate) fn prune_messages_absent_from_remote_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    remote_message_ids: &BTreeSet<MessageId>,
    protected_message_ids: &HashSet<String>,
    force_full_prune: bool,
    affected: &mut ProjectionInputs,
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare("SELECT id FROM message WHERE account_id = ?1")
        .map_err(sql_to_store_error)?;
    let local_message_ids = statement
        .query_map(params![account_id.as_str()], |row| row.get::<_, String>(0))
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?
        .into_iter()
        .map(MessageId)
        .collect::<BTreeSet<_>>();

    if !force_full_prune {
        let local_count = local_message_ids.len();
        if local_count > 0 {
            // Count only the rows this pass would actually DELETE: locals absent
            // from remote and not exempted by the protected set (protected rows
            // never prune, so they must not inflate the floor fraction).
            let would_prune = local_message_ids
                .difference(remote_message_ids)
                .filter(|id| !protected_message_ids.contains(id.as_str()))
                .count();
            let over_floor =
                (would_prune as f64) > (local_count as f64) * MAX_ABSENCE_PRUNE_FRACTION;
            if remote_message_ids.is_empty() || over_floor {
                ph_warn!(
                    posthaste_observability::events::STORE_SYNC_ABSENCE_PRUNE_REFUSED,
                    account_id = %account_id.as_str(),
                    local_count,
                    remote_count = remote_message_ids.len(),
                    would_prune,
                    "refusing prune-by-absence: remote set empty or drastically \
                     smaller than local store; local mail preserved"
                );
                return Ok(());
            }
        }
    }

    for message_id in local_message_ids.difference(remote_message_ids) {
        if protected_message_ids.contains(message_id.as_str()) {
            continue;
        }
        delete_message_and_track_projection_inputs(tx, account_id, message_id, affected)?;
        write_through_draft_registry_on_message_delete_tx(tx, account_id, message_id)?;
        events.record(
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            Some(message_id),
            json!({ "messageId": message_id.as_str(), "deleted": true }),
        )?;
    }
    Ok(())
}

/// M69 (D135) sync write-through, upsert half: a synced message that carries a
/// stable draft key (the round-tripped `X-Posthaste-Draft-Id` header, already
/// projected as `message.draft_id`) upserts the draft registry (still the
/// `draft_alias` table until the M73 rename) in the SAME transaction as the
/// message row itself, so the registry can never be torn or stale relative to
/// the projection. This makes the registry the single authority for the
/// stable-key → live-entity mapping: a draft synced from the server / another
/// device / a past session resolves through the registry alone, and an
/// observed rotation (same key, new provider id) repoints it.
///
/// @spec docs/eph/RFC-L2-draft-identity#21-d135--one-authority-the-draft_registry
fn write_through_draft_registry_on_message_upsert_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain_model::MessageRecord,
) -> Result<(), StoreError> {
    let Some(draft_key) = message.draft_id.as_deref() else {
        return Ok(());
    };
    tx.execute_cached(
        "INSERT INTO draft_alias (account_id, draft_key, entity_id)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, draft_key) DO UPDATE SET entity_id = excluded.entity_id",
        params![account_id.as_str(), draft_key, message.id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// M69 (D135) sync write-through, delete half: when sync deletes a message row
/// the registry points at, the mapping is corrected in the SAME transaction.
/// If another projected row still carries the same draft key (a rotation whose
/// new row was already applied — this chunk's deletes run before its upserts,
/// but an earlier chunk or batch may have upserted the successor first), the
/// registry REPOINTS to it; otherwise the draft is confirmed gone and the
/// registry FORGETS the key. Within-batch rotations (delete old + upsert new)
/// pass through a transient forget here and re-register in the upsert loop
/// below, all inside one transaction. Runs only on sync's delete/prune paths —
/// enqueue-time removal on user discard is unchanged until M70.
///
/// @spec docs/eph/RFC-L2-draft-identity#21-d135--one-authority-the-draft_registry
fn write_through_draft_registry_on_message_delete_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    let mut keys_statement = tx
        .prepare_cached("SELECT draft_key FROM draft_alias WHERE account_id = ?1 AND entity_id = ?2")
        .map_err(sql_to_store_error)?;
    let draft_keys = keys_statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;

    for draft_key in draft_keys {
        // The message row is already deleted, so any hit is a surviving
        // projected row for the same stable key — the rotation's successor.
        let survivor = tx
            .query_row_cached(
                "SELECT id FROM message WHERE account_id = ?1 AND draft_id = ?2 LIMIT 1",
                params![account_id.as_str(), draft_key.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        match survivor {
            Some(live_entity_id) => {
                tx.execute_cached(
                    "UPDATE draft_alias SET entity_id = ?3
                     WHERE account_id = ?1 AND draft_key = ?2",
                    params![account_id.as_str(), draft_key.as_str(), live_entity_id],
                )
                .map_err(sql_to_store_error)?;
            }
            None => {
                tx.execute_cached(
                    "DELETE FROM draft_alias WHERE account_id = ?1 AND draft_key = ?2",
                    params![account_id.as_str(), draft_key.as_str()],
                )
                .map_err(sql_to_store_error)?;
            }
        }
    }
    Ok(())
}

/// Final reconciliation pass for a streamed upsert-only sync: prune locals
/// absent from the complete remote id set gathered across all chunks, then
/// commit the cursors that were withheld until the full stream succeeded — all
/// in one transaction. Additions/updates were already applied + published per
/// chunk; this is the deletion correctness boundary.
///
pub(crate) fn reconcile_sync_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    reconciliation: &SyncReconciliation,
) -> Result<Vec<DomainEvent>, StoreError> {
    reconcile_sync_tx_impl(tx, account_id, reconciliation, &no_protected_message_ids())
}

/// Like [`reconcile_sync_tx`], but excludes `protected_message_ids` from the
/// prune-by-absence pass, for the same reason as
/// [`apply_sync_batch_protected_tx`]. Covers the streamed full-snapshot
/// fallback (e.g. JMAP `cannotCalculateChanges`), whose pruning happens here
/// rather than in an in-batch `replace_all_messages` pass.
pub(crate) fn reconcile_sync_protected_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    reconciliation: &SyncReconciliation,
    protected_message_ids: &HashSet<String>,
) -> Result<Vec<DomainEvent>, StoreError> {
    reconcile_sync_tx_impl(tx, account_id, reconciliation, protected_message_ids)
}

fn reconcile_sync_tx_impl(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    reconciliation: &SyncReconciliation,
    protected_message_ids: &HashSet<String>,
) -> Result<Vec<DomainEvent>, StoreError> {
    let estimated = reconciliation.cursors.len()
        + if reconciliation.prune_mailboxes {
            reconciliation.remote_mailbox_ids.len()
        } else {
            0
        }
        + if reconciliation.prune_messages {
            reconciliation.remote_message_ids.len()
        } else {
            0
        };
    let mut events = EventRecorder::with_capacity(tx, account_id, estimated)?;
    let mut affected = ProjectionInputs::default();

    if reconciliation.prune_mailboxes {
        let remote_mailbox_ids: BTreeSet<_> =
            reconciliation.remote_mailbox_ids.iter().cloned().collect();
        prune_mailboxes_absent_from_remote_tx(tx, account_id, &remote_mailbox_ids, &mut events)?;
    }
    if reconciliation.prune_messages {
        let remote_message_ids: BTreeSet<_> =
            reconciliation.remote_message_ids.iter().cloned().collect();
        prune_messages_absent_from_remote_tx(
            tx,
            account_id,
            &remote_message_ids,
            protected_message_ids,
            // No wired explicit-full-resync signal today; the floor guard is
            // always active on this path (DS1).
            false,
            &mut affected,
            &mut events,
        )?;
    }

    for thread_id in affected.threads {
        refresh_thread_projection_tx(tx, account_id, &thread_id)?;
    }
    for conversation_id in affected.conversations {
        refresh_conversation_projection_tx(tx, &conversation_id)?;
    }
    for cursor in &reconciliation.cursors {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }

    Ok(events.into_events())
}

fn estimate_sync_event_count(batch: &SyncBatch) -> usize {
    batch.mailboxes.len()
        + batch.deleted_mailbox_ids.len()
        + batch.deleted_message_ids.len()
        + batch.deleted_imap_message_locations.len()
        + if batch.replace_all_mailboxes {
            batch.mailboxes.len()
        } else {
            0
        }
        + if batch.replace_all_messages {
            batch.messages.len()
        } else {
            0
        }
        + batch.messages.len() * 4
}
