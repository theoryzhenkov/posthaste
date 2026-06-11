use super::*;

pub(crate) fn track_applied_message_projection_inputs(
    affected: &mut ProjectionInputs,
    message: &posthaste_domain::MessageRecord,
    conversation_id: &ConversationId,
    before: &MessageBeforeApply,
) {
    affected.threads.insert(message.source_thread_id.clone());
    affected.conversations.insert(conversation_id.clone());
    if let Some(previous_conversation_id) = &before.conversation_id {
        affected
            .conversations
            .insert(previous_conversation_id.clone());
    }
    for mailbox_id in before.mailboxes.iter().chain(message.mailbox_ids.iter()) {
        affected.mailboxes.insert(mailbox_id.clone());
    }
}

pub(crate) fn append_message_diff_events_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
    conversation_id: &ConversationId,
    before: &MessageBeforeApply,
    events: &mut Vec<DomainEvent>,
) -> Result<(), StoreError> {
    events.push(insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_UPDATED,
        message.mailbox_ids.first(),
        Some(&message.id),
        json!({
            "messageId": message.id.as_str(),
            "sourceThreadId": message.source_thread_id.as_str(),
            "conversationId": conversation_id.as_str(),
            "created": !before.existed
        }),
    )?);

    if !before.existed || before.keywords != message.keywords {
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED,
            message.mailbox_ids.first(),
            Some(&message.id),
            json!({
                "messageId": message.id.as_str(),
                "keywords": message.keywords,
            }),
        )?);
    }

    let current_mailboxes: BTreeSet<_> = message.mailbox_ids.iter().cloned().collect();
    let previous_mailboxes: BTreeSet<_> = before.mailboxes.iter().cloned().collect();
    if !before.existed || current_mailboxes != previous_mailboxes {
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
            message.mailbox_ids.first(),
            Some(&message.id),
            json!({
                "messageId": message.id.as_str(),
                "mailboxIds": message.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
            }),
        )?);
    }

    for mailbox_id in current_mailboxes.difference(&previous_mailboxes) {
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(mailbox_id),
            Some(&message.id),
            json!({
                "messageId": message.id.as_str(),
                "mailboxId": mailbox_id.as_str(),
            }),
        )?);
    }

    Ok(())
}

pub(crate) fn delete_message_and_track_projection_inputs(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    affected: &mut ProjectionInputs,
) -> Result<(), StoreError> {
    let prior_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    let thread_id = tx
        .query_row(
            "SELECT thread_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .map(ThreadId);
    let conversation_id = tx
        .query_row(
            "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten()
        .map(ConversationId);
    delete_message_tx(tx, account_id, message_id)?;
    for mailbox_id in prior_mailboxes {
        affected.mailboxes.insert(mailbox_id);
    }
    if let Some(thread_id) = thread_id {
        affected.threads.insert(thread_id);
    }
    if let Some(conversation_id) = conversation_id {
        affected.conversations.insert(conversation_id);
    }
    Ok(())
}

pub(crate) fn delete_imap_message_location_and_track_projection_inputs(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    location: &ImapMessageLocationKey,
    affected: &mut ProjectionInputs,
    events: &mut Vec<DomainEvent>,
) -> Result<(), StoreError> {
    let before = fetch_message_before_apply_tx(tx, account_id, &location.message_id)?;
    let deleted = tx
        .execute(
            "DELETE FROM imap_message_location
             WHERE account_id = ?1
               AND message_id = ?2
               AND mailbox_id = ?3
               AND uid_validity = ?4
               AND uid = ?5",
            params![
                account_id.as_str(),
                location.message_id.as_str(),
                location.mailbox_id.as_str(),
                location.uid_validity.0,
                location.uid.0,
            ],
        )
        .map_err(sql_to_store_error)?;
    if deleted == 0 || !before.existed {
        return Ok(());
    }

    tx.execute(
        "DELETE FROM message_mailbox
         WHERE account_id = ?1 AND message_id = ?2 AND mailbox_id = ?3",
        params![
            account_id.as_str(),
            location.message_id.as_str(),
            location.mailbox_id.as_str(),
        ],
    )
    .map_err(sql_to_store_error)?;

    let current_mailboxes = fetch_mailbox_ids_tx(tx, account_id, &location.message_id)?;
    let previous_mailboxes: BTreeSet<_> = before.mailboxes.iter().cloned().collect();
    let current_mailbox_set: BTreeSet<_> = current_mailboxes.iter().cloned().collect();
    if current_mailbox_set == previous_mailboxes {
        return Ok(());
    }

    for mailbox_id in previous_mailboxes.iter().chain(current_mailbox_set.iter()) {
        affected.mailboxes.insert(mailbox_id.clone());
    }
    events.push(insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
        Some(&location.mailbox_id),
        Some(&location.message_id),
        json!({
            "messageId": location.message_id.as_str(),
            "mailboxIds": current_mailboxes.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
        }),
    )?);
    Ok(())
}
