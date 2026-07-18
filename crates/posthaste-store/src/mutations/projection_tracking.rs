use super::*;
use crate::sql_cache::CachedSql;

pub(crate) fn track_applied_message_projection_inputs(
    affected: &mut ProjectionInputs,
    message: &posthaste_domain_model::MessageRecord,
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
    message: &posthaste_domain_model::MessageRecord,
    conversation_id: &ConversationId,
    before: &MessageBeforeApply,
    projection: Option<&posthaste_domain_model::MessageSummary>,
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let diff = MessageEventDiff::new(message, conversation_id, before, projection);

    events.record(
        EVENT_TOPIC_MESSAGE_UPDATED,
        diff.primary_mailbox(),
        Some(&message.id),
        diff.message_updated_payload(),
    )?;

    Ok(())
}

struct MessageEventDiff<'a> {
    message: &'a posthaste_domain_model::MessageRecord,
    conversation_id: &'a ConversationId,
    before: &'a MessageBeforeApply,
    projection: Option<&'a posthaste_domain_model::MessageSummary>,
    current_mailboxes: BTreeSet<MailboxId>,
    previous_mailboxes: BTreeSet<MailboxId>,
}

impl<'a> MessageEventDiff<'a> {
    fn new(
        message: &'a posthaste_domain_model::MessageRecord,
        conversation_id: &'a ConversationId,
        before: &'a MessageBeforeApply,
        projection: Option<&'a posthaste_domain_model::MessageSummary>,
    ) -> Self {
        Self {
            message,
            conversation_id,
            before,
            projection,
            current_mailboxes: message.mailbox_ids.iter().cloned().collect(),
            previous_mailboxes: before.mailboxes.iter().cloned().collect(),
        }
    }

    fn primary_mailbox(&self) -> Option<&MailboxId> {
        self.message.mailbox_ids.first()
    }

    // The typed contract lives in `posthaste_domain_model::MessageUpdatedPayload`
    // (mirrored into the client protocol). The full `MessageSummary` projection
    // rides along — enough for the reactive store to materialize a never-held
    // message (sort key, row key, membership, render) without a promotion
    // round-trip (`firehose-carries-rows`). Byte-identical to a served row (one
    // derivation). The flat keyword/mailboxIds fields are retained for the
    // legacy invalidation path until 2e retires it. No counts on the event
    // (RFC-L2-count-unification): clients invalidate their mailbox-count
    // queries on `message.updated` and re-read the trigger-maintained
    // canonical counts instead of applying deltas.
    fn message_updated_payload(&self) -> Value {
        let arrived_mailbox_ids: Vec<MailboxId> = self
            .current_mailboxes
            .difference(&self.previous_mailboxes)
            .cloned()
            .collect();
        let payload = posthaste_domain_model::MessageUpdatedPayload {
            message_id: self.message.id.clone(),
            source_thread_id: self.message.source_thread_id.clone(),
            conversation_id: self.conversation_id.clone(),
            created: !self.before.existed,
            changes: posthaste_domain_model::MessageChangeFlags {
                keywords: self.keywords_changed(),
                mailboxes: self.mailboxes_changed(),
                arrived: !arrived_mailbox_ids.is_empty(),
            },
            keywords: self.message.keywords.clone(),
            mailbox_ids: self.message.mailbox_ids.clone(),
            arrived_mailbox_ids,
            projection: self.projection.cloned(),
        };
        serde_json::to_value(&payload).unwrap_or(Value::Null)
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
    // Tear down the sync-owned IMAP coordinates here, on the server-confirmed
    // (VANISHED/absence) delete path — NOT in `delete_message_tx`, so the
    // optimistic Destroy write-through leaves them intact for the outbox flush
    // to read back and issue the server-side delete (DP-C1). This is the
    // catch-all for any coordinate not already pruned via the batch's explicit
    // `deleted_imap_message_locations`.
    tx.execute(
        "DELETE FROM imap_message_location WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
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

    // Attach the post-removal projection, like the command path: the reactive
    // store drops projection-less events, so without it the store cannot
    // self-maintain membership for an IMAP expunge/location-removal and the row
    // only corrected on a full re-serve. Counts ride no event — clients
    // invalidate + re-read the canonical mailbox counts.
    let detail = query_message_detail_tx(tx, account_id, &location.message_id)?;
    let payload = json!({
        "messageId": location.message_id.as_str(),
        "changes": { "mailboxes": true },
        "mailboxIds": current_mailboxes.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
        "removedMailboxId": location.mailbox_id.as_str(),
        "projection": detail.as_ref().map(|detail| &detail.summary),
    });
    events.record(
        EVENT_TOPIC_MESSAGE_UPDATED,
        current_mailboxes.first(),
        Some(&location.message_id),
        payload,
    )?;
    Ok(())
}
