use super::*;

pub(crate) fn prune_stale_imap_message_locations_for_snapshot_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    remote_locations: &[ImapMessageLocation],
) -> Result<(), StoreError> {
    let remote_keys = remote_locations
        .iter()
        .map(|location| (location.message_id.clone(), location.mailbox_id.clone()))
        .collect::<BTreeSet<_>>();
    let local_keys = tx
        .prepare(
            "SELECT message_id, mailbox_id
             FROM imap_message_location
             WHERE account_id = ?1",
        )
        .map_err(sql_to_store_error)?
        .query_map(params![account_id.as_str()], |row| {
            Ok((MessageId(row.get(0)?), MailboxId(row.get(1)?)))
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;

    for (message_id, mailbox_id) in local_keys {
        if remote_keys.contains(&(message_id.clone(), mailbox_id.clone())) {
            continue;
        }
        tx.execute(
            "DELETE FROM imap_message_location
             WHERE account_id = ?1 AND message_id = ?2 AND mailbox_id = ?3",
            params![
                account_id.as_str(),
                message_id.as_str(),
                mailbox_id.as_str()
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

pub(crate) fn delete_mailbox_and_track_projection_inputs(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
    affected: &mut ProjectionInputs,
    events: &mut Vec<DomainEvent>,
) -> Result<(), StoreError> {
    let message_ids = tx
        .prepare(
            "SELECT message_id FROM message_mailbox
             WHERE account_id = ?1 AND mailbox_id = ?2",
        )
        .map_err(sql_to_store_error)?
        .query_map(params![account_id.as_str(), mailbox_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?
        .into_iter()
        .map(MessageId)
        .collect::<Vec<_>>();

    tx.execute(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND mailbox_id = ?2",
        params![account_id.as_str(), mailbox_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM imap_mailbox_sync_state WHERE account_id = ?1 AND mailbox_id = ?2",
        params![account_id.as_str(), mailbox_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM imap_message_location WHERE account_id = ?1 AND mailbox_id = ?2",
        params![account_id.as_str(), mailbox_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM mailbox_role_override WHERE account_id = ?1 AND mailbox_id = ?2",
        params![account_id.as_str(), mailbox_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM mailbox WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), mailbox_id.as_str()],
    )
    .map_err(sql_to_store_error)?;

    affected.mailboxes.insert(mailbox_id.clone());
    for message_id in message_ids {
        let mailbox_ids = fetch_mailbox_ids_tx(tx, account_id, &message_id)?;
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
            mailbox_ids.first(),
            Some(&message_id),
            json!({
                "messageId": message_id.as_str(),
                "mailboxIds": mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
            }),
        )?);
    }
    Ok(())
}
