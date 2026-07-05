use super::*;

/// Delete a message row and every LOCAL junction/cache keyed to it.
///
/// Deliberately does NOT touch `imap_message_location`: those provider
/// coordinates (uid_validity/uid per mailbox) are sync-owned and are the exact
/// keys the outbox flush reads back to issue the server-side IMAP delete
/// (`UID STORE \Deleted` + `UID EXPUNGE`). Wiping them in the optimistic
/// write-through (as this used to do) left the flush with no coordinates → the
/// destroy op was `Rejected`→`Failed`, the server delete never issued, and the
/// next IMAP delta re-imported the still-live UID as new mail — the message
/// resurrected on every hard-delete (DP-C1).
///
/// The coordinates are torn down exactly once, later, by the sync-owned delete
/// path: [`delete_message_and_track_projection_inputs`](crate::mutations::delete_message_and_track_projection_inputs)
/// (a server-confirmed VANISHED/absence removal) and the CONDSTORE
/// `deleted_imap_message_locations` prune. Until then the message row is gone
/// (optimistic hide is immediate), while the coordinates survive for the flush.
pub(crate) fn delete_message_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM cache_rescore_queue WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM cache_message_signal WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM cache_object WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_body WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_attachment WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM conversation_message WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}
