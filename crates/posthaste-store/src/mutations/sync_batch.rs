use super::*;
use crate::sql_cache::CachedSql;

pub(crate) fn stage_sync_bodies(
    store: &DatabaseStore,
    account_id: &AccountId,
    batch: &SyncBatch,
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
                .map(|raw_mime| store.store_raw_message(account_id, raw_mime))
                .transpose()
        })
        .collect()
}

pub(crate) fn apply_sync_batch_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged_bodies: &[Option<RawMessageRef>],
) -> Result<Vec<DomainEvent>, StoreError> {
    let mut events = Vec::new();
    let mut affected = ProjectionInputs::default();

    if batch.replace_all_mailboxes {
        let remote_mailbox_ids: BTreeSet<_> = batch
            .mailboxes
            .iter()
            .map(|mailbox| mailbox.id.clone())
            .collect();
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

        for mailbox_id in local_mailbox_ids.difference(&remote_mailbox_ids) {
            delete_mailbox_and_track_projection_inputs(tx, account_id, mailbox_id, &mut events)?;
            events.push(insert_event_tx(
                tx,
                account_id,
                EVENT_TOPIC_MAILBOX_UPDATED,
                Some(mailbox_id),
                None,
                json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
            )?);
        }
    }

    if batch.replace_all_messages {
        let remote_message_ids: BTreeSet<_> = batch
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect();
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

        for message_id in local_message_ids.difference(&remote_message_ids) {
            delete_message_and_track_projection_inputs(tx, account_id, message_id, &mut affected)?;
            events.push(insert_event_tx(
                tx,
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                None,
                Some(message_id),
                json!({ "messageId": message_id.as_str(), "deleted": true }),
            )?);
        }

        prune_stale_imap_message_locations_for_snapshot_tx(
            tx,
            account_id,
            &batch.imap_message_locations,
        )?;
    }

    for mailbox_id in &batch.deleted_mailbox_ids {
        delete_mailbox_and_track_projection_inputs(tx, account_id, mailbox_id, &mut events)?;
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
        )?);
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
        delete_message_and_track_projection_inputs(tx, account_id, message_id, &mut affected)?;
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            Some(message_id),
            json!({ "messageId": message_id.as_str(), "deleted": true }),
        )?);
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
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(&mailbox.id),
            None,
            json!({ "mailboxId": mailbox.id.as_str() }),
        )?);
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
                    highest_uid, highest_modseq, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, mailbox_id) DO UPDATE SET
                    mailbox_name = excluded.mailbox_name,
                    uid_validity = excluded.uid_validity,
                    highest_uid = excluded.highest_uid,
                    highest_modseq = excluded.highest_modseq,
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

    Ok(events)
}
