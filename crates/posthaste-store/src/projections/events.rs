use super::*;
use crate::sql_cache::CachedSql;

/// Records a group of related events with one shared timestamp.
pub(crate) struct EventRecorder<'tx, 'conn, 'account> {
    tx: &'tx Transaction<'conn>,
    account_id: &'account AccountId,
    occurred_at: String,
    events: Vec<DomainEvent>,
}

impl<'tx, 'conn, 'account> EventRecorder<'tx, 'conn, 'account> {
    pub(crate) fn with_capacity(
        tx: &'tx Transaction<'conn>,
        account_id: &'account AccountId,
        capacity: usize,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            tx,
            account_id,
            occurred_at: now_iso8601()?,
            events: Vec::with_capacity(capacity),
        })
    }

    pub(crate) fn record(
        &mut self,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: Value,
    ) -> Result<(), StoreError> {
        let event = insert_event_at_tx(
            self.tx,
            self.account_id,
            topic,
            mailbox_id,
            message_id,
            payload,
            &self.occurred_at,
        )?;
        self.events.push(event);
        Ok(())
    }

    pub(crate) fn into_events(self) -> Vec<DomainEvent> {
        self.events
    }
}

/// Inserts a domain event into `event_log` with a monotonically increasing
/// `seq`.
///
/// @spec docs/L1-sync#event-propagation
pub(crate) fn insert_event_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    topic: &str,
    mailbox_id: Option<&MailboxId>,
    message_id: Option<&MessageId>,
    payload: Value,
) -> Result<DomainEvent, StoreError> {
    let occurred_at = now_iso8601()?;
    insert_event_at_tx(
        tx,
        account_id,
        topic,
        mailbox_id,
        message_id,
        payload,
        &occurred_at,
    )
}

pub(crate) fn insert_event_at_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    topic: &str,
    mailbox_id: Option<&MailboxId>,
    message_id: Option<&MessageId>,
    payload: Value,
    occurred_at: &str,
) -> Result<DomainEvent, StoreError> {
    tx.execute_cached(
        "INSERT INTO event_log (account_id, topic, occurred_at, mailbox_id, message_id, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account_id.as_str(),
            topic,
            occurred_at,
            mailbox_id.map(MailboxId::as_str),
            message_id.map(MessageId::as_str),
            payload.to_string()
        ],
    )
    .map_err(sql_to_store_error)?;
    let seq = tx.last_insert_rowid();
    Ok(DomainEvent {
        seq,
        account_id: account_id.clone(),
        topic: topic.to_string(),
        occurred_at: occurred_at.to_string(),
        mailbox_id: mailbox_id.cloned(),
        message_id: message_id.cloned(),
        payload,
    })
}
