use super::*;
use crate::sql_cache::CachedSql;

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
}

pub(crate) fn append_message_diff_events_tx(
    message: &posthaste_domain::MessageRecord,
    conversation_id: &ConversationId,
    before: &MessageBeforeApply,
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let diff = MessageEventDiff::new(message, conversation_id, before);

    events.record(
        EVENT_TOPIC_MESSAGE_UPDATED,
        diff.primary_mailbox(),
        Some(&message.id),
        diff.message_updated_payload(),
    )?;

    Ok(())
}

struct MessageEventDiff<'a> {
    message: &'a posthaste_domain::MessageRecord,
    conversation_id: &'a ConversationId,
    before: &'a MessageBeforeApply,
    current_mailboxes: BTreeSet<MailboxId>,
    previous_mailboxes: BTreeSet<MailboxId>,
}

impl<'a> MessageEventDiff<'a> {
    fn new(
        message: &'a posthaste_domain::MessageRecord,
        conversation_id: &'a ConversationId,
        before: &'a MessageBeforeApply,
    ) -> Self {
        Self {
            message,
            conversation_id,
            before,
            current_mailboxes: message.mailbox_ids.iter().cloned().collect(),
            previous_mailboxes: before.mailboxes.iter().cloned().collect(),
        }
    }

    fn primary_mailbox(&self) -> Option<&MailboxId> {
        self.message.mailbox_ids.first()
    }

    fn message_updated_payload(&self) -> Value {
        let arrived_mailbox_ids = self
            .current_mailboxes
            .difference(&self.previous_mailboxes)
            .map(MailboxId::as_str)
            .collect::<Vec<_>>();
        json!({
            "messageId": self.message.id.as_str(),
            "sourceThreadId": self.message.source_thread_id.as_str(),
            "conversationId": self.conversation_id.as_str(),
            "created": !self.before.existed,
            "changes": {
                "keywords": self.keywords_changed(),
                "mailboxes": self.mailboxes_changed(),
                "arrived": !arrived_mailbox_ids.is_empty(),
            },
            "keywords": self.message.keywords,
            "mailboxIds": self.message.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
            "arrivedMailboxIds": arrived_mailbox_ids,
        })
    }

    fn keywords_changed(&self) -> bool {
        !self.before.existed || self.before.keywords != self.message.keywords
    }

    fn mailboxes_changed(&self) -> bool {
        !self.before.existed || self.current_mailboxes != self.previous_mailboxes
    }
}

pub(crate) fn delete_message_and_track_projection_inputs(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    affected: &mut ProjectionInputs,
) -> Result<(), StoreError> {
    let thread_id = tx
        .query_row_cached(
            "SELECT thread_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .map(ThreadId);
    let conversation_id = tx
        .query_row_cached(
            "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten()
        .map(ConversationId);
    delete_message_tx(tx, account_id, message_id)?;
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
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let before = fetch_message_before_apply_tx(tx, account_id, &location.message_id)?;
    let deleted = tx
        .execute_cached(
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

    tx.execute_cached(
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

    events.record(
        EVENT_TOPIC_MESSAGE_UPDATED,
        current_mailboxes.first(),
        Some(&location.message_id),
        json!({
            "messageId": location.message_id.as_str(),
            "changes": { "mailboxes": true },
            "mailboxIds": current_mailboxes.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
            "removedMailboxId": location.mailbox_id.as_str(),
        }),
    )?;
    Ok(())
}
