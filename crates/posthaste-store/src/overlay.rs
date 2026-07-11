use super::*;

use crate::projections::normalized_subject;
use crate::sql_cache::CachedSql;

/// NS1 (D167): the overlay plane's SQLite backend. See the port doc
/// (`posthaste-domain-service/src/ports/overlay_store.rs`) and the schema
/// comment on `message_overlay` for the plane contract. Everything here writes
/// the `*_overlay` tables ONLY — never `message` / `message_mailbox` /
/// `message_keyword`, which stay sync-owned.
impl MessageOverlayStore for DatabaseStore {
    /// One transaction: the folded row (mirroring the sync upsert's column
    /// mapping, incl. `is_read`/`is_flagged` derived from `$seen`/`$flagged`)
    /// plus full membership/keyword set replacement. `conversation_id` is
    /// copied from the base row when one exists (a folded edit of a synced
    /// message keeps its conversation); an overlay-only row (a pending draft
    /// create) falls back to its own id — provisional, replaced by the real
    /// conversation assignment when the row reconciles into base.
    fn upsert_overlay_message(
        &self,
        account_id: &AccountId,
        message: &posthaste_domain_model::MessageRecord,
    ) -> Result<(), StoreError> {
        let seen = message.keywords.iter().any(|keyword| keyword == "$seen");
        let flagged = message.keywords.iter().any(|keyword| keyword == "$flagged");
        let to_json = serde_json::to_string(&message.to).map_err(json_to_store_error)?;
        let references_json =
            serde_json::to_string(&message.references).map_err(json_to_store_error)?;
        let list_unsubscribe = message
            .list_unsubscribe
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(json_to_store_error)?;
        self.write_transaction(|tx| {
            tx.execute_cached(
                "INSERT INTO message_overlay (
                    account_id, id, thread_id, conversation_id, remote_blob_id, subject,
                    normalized_subject, from_name, from_email, to_json, preview, received_at,
                    has_attachment, size, is_read, is_flagged, rfc_message_id, in_reply_to,
                    references_json, draft_id, list_unsubscribe, tombstone
                 ) VALUES (
                    ?1, ?2, ?3,
                    COALESCE((SELECT conversation_id FROM message
                              WHERE account_id = ?1 AND id = ?2), ?2),
                    ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, 0
                 )
                 ON CONFLICT(account_id, id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    conversation_id = excluded.conversation_id,
                    remote_blob_id = excluded.remote_blob_id,
                    subject = excluded.subject,
                    normalized_subject = excluded.normalized_subject,
                    from_name = excluded.from_name,
                    from_email = excluded.from_email,
                    to_json = excluded.to_json,
                    preview = excluded.preview,
                    received_at = excluded.received_at,
                    has_attachment = excluded.has_attachment,
                    size = excluded.size,
                    is_read = excluded.is_read,
                    is_flagged = excluded.is_flagged,
                    rfc_message_id = excluded.rfc_message_id,
                    in_reply_to = excluded.in_reply_to,
                    references_json = excluded.references_json,
                    draft_id = excluded.draft_id,
                    list_unsubscribe = excluded.list_unsubscribe,
                    tombstone = 0",
                params![
                    account_id.as_str(),
                    message.id.as_str(),
                    message.source_thread_id.as_str(),
                    message
                        .remote_blob_id
                        .as_ref()
                        .map(|blob_id| blob_id.as_str()),
                    message.subject,
                    normalized_subject(message.subject.as_deref()),
                    message.from_name,
                    message.from_email,
                    to_json,
                    message.preview,
                    message.received_at,
                    i64::from(message.has_attachment),
                    message.size,
                    i64::from(seen),
                    i64::from(flagged),
                    message.rfc_message_id,
                    message.in_reply_to,
                    references_json,
                    message.draft_id,
                    list_unsubscribe,
                ],
            )
            .map_err(sql_to_store_error)?;
            replace_overlay_sets_tx(tx, account_id, &message.id, |insert_mailbox, insert_keyword| {
                for mailbox_id in &message.mailbox_ids {
                    insert_mailbox(mailbox_id.as_str())?;
                }
                for keyword in &message.keywords {
                    insert_keyword(keyword)?;
                }
                Ok(())
            })
        })
    }

    /// A pending Destroy: keep (or create) the overlay row with `tombstone=1`
    /// and empty sets, hiding the message from every `_effective` view while
    /// base still holds it. The minimal placeholder columns matter only until
    /// retire — a tombstoned row is excluded from `message_effective`, so its
    /// values are never read.
    fn tombstone_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute_cached(
                "INSERT INTO message_overlay (account_id, id, thread_id, received_at, tombstone)
                 VALUES (?1, ?2, '', '', 1)
                 ON CONFLICT(account_id, id) DO UPDATE SET tombstone = 1",
                params![account_id.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            replace_overlay_sets_tx(tx, account_id, message_id, |_, _| Ok(()))
        })
    }

    fn remove_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute_cached(
                "DELETE FROM message_overlay WHERE account_id = ?1 AND id = ?2",
                params![account_id.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            replace_overlay_sets_tx(tx, account_id, message_id, |_, _| Ok(()))
        })
    }

    fn list_overlay_message_ids(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MessageId>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached("SELECT id FROM message_overlay WHERE account_id = ?1 ORDER BY id")
            .map_err(sql_to_store_error)?;
        let ids = statement
            .query_map(params![account_id.as_str()], |row| {
                row.get::<_, String>(0).map(MessageId)
            })
            .map_err(sql_to_store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(ids)
    }
}

/// Clears both overlay set tables for the message, then hands the caller
/// per-value inserters to rebuild them. Passing a no-op rebuild (`|_, _|
/// Ok(())`) just clears — the tombstone/remove paths.
fn replace_overlay_sets_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    rebuild: impl FnOnce(
        &mut dyn FnMut(&str) -> Result<(), StoreError>,
        &mut dyn FnMut(&str) -> Result<(), StoreError>,
    ) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_mailbox_overlay WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute_cached(
        "DELETE FROM message_keyword_overlay WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    let mut insert_mailbox = |mailbox_id: &str| -> Result<(), StoreError> {
        tx.execute_cached(
            "INSERT OR IGNORE INTO message_mailbox_overlay (account_id, message_id, mailbox_id)
             VALUES (?1, ?2, ?3)",
            params![account_id.as_str(), message_id.as_str(), mailbox_id],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    };
    let mut insert_keyword = |keyword: &str| -> Result<(), StoreError> {
        tx.execute_cached(
            "INSERT OR IGNORE INTO message_keyword_overlay (account_id, message_id, keyword)
             VALUES (?1, ?2, ?3)",
            params![account_id.as_str(), message_id.as_str(), keyword],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    };
    rebuild(&mut insert_mailbox, &mut insert_keyword)
}
