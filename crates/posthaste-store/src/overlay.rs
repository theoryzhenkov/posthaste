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
        self.write_transaction(|tx| upsert_overlay_tx(tx, account_id, message))
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
        self.write_transaction(|tx| tombstone_overlay_tx(tx, account_id, message_id))
    }

    fn remove_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| remove_overlay_tx(tx, account_id, message_id))
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

    fn read_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<Option<posthaste_domain_model::MessageRecord>>, StoreError> {
        let connection = self.read_connection()?;
        read_overlay_on(&connection, account_id, message_id)
    }

    fn find_base_message_id_by_rfc_prefix(
        &self,
        account_id: &AccountId,
        prefix: &str,
    ) -> Result<Option<MessageId>, StoreError> {
        let connection = self.read_connection()?;
        // Escape LIKE metacharacters in the token (`_` appears in sanitized
        // op ids). Unindexed scan — acceptable: this probe runs only while a
        // provisional Sent entry exists (short-lived, low cardinality).
        let escaped = prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        connection
            .query_row(
                "SELECT id FROM message
                 WHERE account_id = ?1 AND rfc_message_id LIKE ?2 || '%' ESCAPE '\\'
                 LIMIT 1",
                params![account_id.as_str(), escaped],
                |row| row.get::<_, String>(0).map(MessageId),
            )
            .optional()
            .map_err(sql_to_store_error)
    }

    fn read_base_message_record(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<posthaste_domain_model::MessageRecord>, StoreError> {
        let connection = self.read_connection()?;
        read_base_on(&connection, account_id, message_id)
    }

    /// Derive one row's overlay entry atomically: one write transaction
    /// snapshots `base` + the unsettled log + the draft-key map, runs `fold`
    /// (which reads ONLY from the snapshot), applies the mutation, and returns
    /// the visibility diff. SQLite serializes writers, so this is the single
    /// point that makes `overlay = replay(log, base)` hold under concurrency —
    /// no concurrent base write or sibling refresh can interleave.
    fn derive_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        fold: OverlayFold,
    ) -> Result<DeriveDiff, StoreError> {
        self.write_transaction(|tx| {
            let base = read_base_on(tx, account_id, message_id)?;
            let overlay = read_overlay_on(tx, account_id, message_id)?;
            let ops = crate::outbox::collect_operations(
                tx,
                &format!(
                    "SELECT {} FROM outbox_operation
                     WHERE account_id = ?1
                       AND (state != 'failed'
                            OR kind IN ({content_kinds}))
                     ORDER BY rowid ASC
                     LIMIT ?2",
                    crate::outbox::OPERATION_COLUMNS,
                    content_kinds = crate::outbox::content_op_kinds_in_sql()
                ),
                account_id,
                crate::outbox::OUTBOX_LIST_SAFETY_LIMIT,
            )?;
            let draft_keys = read_draft_alias_map_on(tx, account_id)?;
            let drafts_mailbox = read_mailbox_id_by_role_on(tx, account_id, "drafts")?;
            let sent_mailbox = read_mailbox_id_by_role_on(tx, account_id, "sent")?;
            let base_present = base.is_some();
            let was_visible = overlay.as_ref().is_some_and(|entry| entry.is_some());
            let was_effective =
                overlay_effective_visible(overlay.as_ref().map(|o| o.as_ref()), base_present);
            let snapshot = DeriveSnapshot {
                base,
                overlay,
                ops,
                draft_keys,
                drafts_mailbox,
                sent_mailbox,
            };
            let mutation = fold(&snapshot)?;
            let (now_visible, now_effective) = apply_overlay_mutation_tx(
                tx,
                account_id,
                message_id,
                mutation,
                was_visible,
                was_effective,
                base_present,
            )?;
            Ok(DeriveDiff {
                was_visible,
                now_visible,
                was_effective,
                now_effective,
            })
        })
    }

    fn remove_op_and_derive(
        &self,
        account_id: &AccountId,
        op_id: &posthaste_domain_model::OperationId,
        row_ids: &[MessageId],
        fold: OverlayFoldMany,
    ) -> Result<Vec<DeriveDiff>, StoreError> {
        self.write_transaction(|tx| {
            crate::outbox::remove_operation_tx(tx, op_id)?;
            // Read the log + draft-key map + role mailboxes once, post-removal,
            // so every row's fold sees the op already gone.
            let draft_keys = read_draft_alias_map_on(tx, account_id)?;
            let drafts_mailbox = read_mailbox_id_by_role_on(tx, account_id, "drafts")?;
            let sent_mailbox = read_mailbox_id_by_role_on(tx, account_id, "sent")?;
            let ops = crate::outbox::collect_operations(
                tx,
                &format!(
                    "SELECT {} FROM outbox_operation
                     WHERE account_id = ?1
                       AND (state != 'failed'
                            OR kind IN ({content_kinds}))
                     ORDER BY rowid ASC
                     LIMIT ?2",
                    crate::outbox::OPERATION_COLUMNS,
                    content_kinds = crate::outbox::content_op_kinds_in_sql()
                ),
                account_id,
                crate::outbox::OUTBOX_LIST_SAFETY_LIMIT,
            )?;
            let mut diffs = Vec::with_capacity(row_ids.len());
            for row_id in row_ids {
                let base = read_base_on(tx, account_id, row_id)?;
                let overlay = read_overlay_on(tx, account_id, row_id)?;
                let base_present = base.is_some();
                let was_visible = overlay.as_ref().is_some_and(|entry| entry.is_some());
                let was_effective =
                    overlay_effective_visible(overlay.as_ref().map(|o| o.as_ref()), base_present);
                let snapshot = DeriveSnapshot {
                    base: base.clone(),
                    overlay: overlay.clone(),
                    ops: ops.clone(),
                    draft_keys: draft_keys.clone(),
                    drafts_mailbox: drafts_mailbox.clone(),
                    sent_mailbox: sent_mailbox.clone(),
                };
                let mutation = fold(row_id, &snapshot)?;
                let (now_visible, now_effective) = apply_overlay_mutation_tx(
                    tx,
                    account_id,
                    row_id,
                    mutation,
                    was_visible,
                    was_effective,
                    base_present,
                )?;
                diffs.push(DeriveDiff {
                    was_visible,
                    now_visible,
                    was_effective,
                    now_effective,
                });
            }
            Ok(diffs)
        })
    }
}

/// Transactional variants of the overlay read/write methods, shared by the
/// trait methods and [`MessageOverlayStore::derive_overlay`]. They take a
/// `&Transaction` (writes) or `&Connection` (reads — `Transaction` derefs to
/// `Connection`) so the derive can snapshot base + the log + the draft-key
/// map, fold, and apply the mutation inside ONE write transaction.
///
/// A row's EFFECTIVE visibility from its overlay entry and base presence: a
/// folded overlay row serves it; a tombstone hides base; an absent overlay
/// lets base show through. Used to compute the derive's visibility diff.
fn overlay_effective_visible(
    overlay: Option<Option<&posthaste_domain_model::MessageRecord>>,
    base_present: bool,
) -> bool {
    match overlay {
        Some(Some(_)) => true,
        Some(None) => false,
        None => base_present,
    }
}

/// Apply one fold mutation to a row's overlay entry inside the derive
/// transaction, returning the row's resulting `(now_visible, now_effective)`
/// — `Keep` preserves the prior values; `Upsert` is visible (and effective);
/// `Tombstone` hides the row and base (not effective); `Remove` drops the
/// overlay entry (not visible) and is effective iff `base_present` (base shows
/// through). Shared by `derive_overlay` and `remove_op_and_derive` so the
/// mutation→write mapping lives in exactly one place.
fn apply_overlay_mutation_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    mutation: OverlayMutation,
    was_visible: bool,
    was_effective: bool,
    base_present: bool,
) -> Result<(bool, bool), StoreError> {
    let (now_visible, now_effective) = match &mutation {
        OverlayMutation::Upsert(_) => (true, true),
        OverlayMutation::Tombstone => (false, false),
        OverlayMutation::Remove => (false, base_present),
        OverlayMutation::Keep => (was_visible, was_effective),
    };
    match mutation {
        OverlayMutation::Upsert(record) => upsert_overlay_tx(tx, account_id, &record)?,
        OverlayMutation::Tombstone => tombstone_overlay_tx(tx, account_id, message_id)?,
        OverlayMutation::Remove => remove_overlay_tx(tx, account_id, message_id)?,
        OverlayMutation::Keep => {}
    }
    Ok((now_visible, now_effective))
}

fn upsert_overlay_tx(
    tx: &Transaction<'_>,
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
    replace_overlay_sets_tx(
        tx,
        account_id,
        &message.id,
        |insert_mailbox, insert_keyword| {
            for mailbox_id in &message.mailbox_ids {
                insert_mailbox(mailbox_id.as_str())?;
            }
            for keyword in &message.keywords {
                insert_keyword(keyword)?;
            }
            Ok(())
        },
    )
}

fn tombstone_overlay_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "INSERT INTO message_overlay (account_id, id, thread_id, received_at, tombstone)
         VALUES (?1, ?2, '', '', 1)
         ON CONFLICT(account_id, id) DO UPDATE SET tombstone = 1",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    replace_overlay_sets_tx(tx, account_id, message_id, |_, _| Ok(()))
}

fn remove_overlay_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_overlay WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    replace_overlay_sets_tx(tx, account_id, message_id, |_, _| Ok(()))
}

fn read_overlay_on(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<Option<posthaste_domain_model::MessageRecord>>, StoreError> {
    let row = connection
        .query_row(
            "SELECT tombstone, thread_id, subject, from_name, from_email, received_at,
                    draft_id, rfc_message_id
             FROM message_overlay
             WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()
        .map_err(sql_to_store_error)?;
    let Some((
        tombstone,
        thread_id,
        subject,
        from_name,
        from_email,
        received_at,
        draft_id,
        rfc_message_id,
    )) = row
    else {
        return Ok(None);
    };
    if tombstone != 0 {
        return Ok(Some(None));
    }
    let mut mailbox_statement = connection
        .prepare_cached(
            "SELECT mailbox_id FROM message_mailbox_overlay
             WHERE account_id = ?1 AND message_id = ?2 ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let mailbox_ids = mailbox_statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get::<_, String>(0).map(MailboxId)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    let mut keyword_statement = connection
        .prepare_cached(
            "SELECT keyword FROM message_keyword_overlay
             WHERE account_id = ?1 AND message_id = ?2 ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let keywords = keyword_statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(Some(Some(posthaste_domain_model::MessageRecord {
        id: message_id.clone(),
        source_thread_id: ThreadId(thread_id),
        subject,
        from_name,
        from_email,
        received_at,
        mailbox_ids,
        keywords,
        // Identity columns the retire/adoption logic keys on: `draft_id`
        // discriminates the draft-aware keyword compare; `rfc_message_id`
        // carries the provisional Sent row's adoption token.
        draft_id,
        rfc_message_id,
        ..Default::default()
    })))
}

fn read_base_on(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<posthaste_domain_model::MessageRecord>, StoreError> {
    let row = connection
        .query_row(
            "SELECT thread_id, remote_blob_id, subject, from_name, from_email, to_json,
                    preview, received_at, has_attachment, size, rfc_message_id, in_reply_to,
                    references_json, draft_id, list_unsubscribe
             FROM message
             WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                ))
            },
        )
        .optional()
        .map_err(sql_to_store_error)?;
    let Some((
        thread_id,
        remote_blob_id,
        subject,
        from_name,
        from_email,
        to_json,
        preview,
        received_at,
        has_attachment,
        size,
        rfc_message_id,
        in_reply_to,
        references_json,
        draft_id,
        list_unsubscribe,
    )) = row
    else {
        return Ok(None);
    };
    // Base-plane sets, deliberately NOT the `_effective` views: this is
    // the fold's input, so it must be raw provider truth.
    let mut mailbox_statement = connection
        .prepare_cached(
            "SELECT mailbox_id FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2 ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let mailbox_ids = mailbox_statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get::<_, String>(0).map(MailboxId)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    let mut keyword_statement = connection
        .prepare_cached(
            "SELECT keyword FROM message_keyword
             WHERE account_id = ?1 AND message_id = ?2 ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let keywords = keyword_statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(Some(posthaste_domain_model::MessageRecord {
        id: message_id.clone(),
        source_thread_id: ThreadId(thread_id),
        remote_blob_id: remote_blob_id.map(posthaste_domain_model::BlobId),
        subject,
        from_name,
        from_email,
        to: serde_json::from_str(&to_json).map_err(json_to_store_error)?,
        preview,
        received_at,
        has_attachment: has_attachment != 0,
        size,
        mailbox_ids,
        keywords,
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id,
        in_reply_to,
        references: serde_json::from_str(&references_json).map_err(json_to_store_error)?,
        draft_id,
        // Degrades to "no target" on schema drift, mirroring the detail read.
        list_unsubscribe: list_unsubscribe.and_then(|json| serde_json::from_str(&json).ok()),
    }))
}

/// The account's draft-key → live-entity-id map (`draft_alias`), read on the
/// same transaction as the fold's other inputs so a concurrent registry
/// rotation cannot interleave. Small (one row per active draft key).
fn read_draft_alias_map_on(
    connection: &Connection,
    account_id: &AccountId,
) -> Result<std::collections::HashMap<String, String>, StoreError> {
    let mut statement = connection
        .prepare("SELECT draft_key, entity_id FROM draft_alias WHERE account_id = ?1")
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_to_store_error)?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (key, entity_id) = row.map_err(sql_to_store_error)?;
        map.insert(key, entity_id);
    }
    Ok(map)
}

/// A role-mailbox id (Drafts / Sent), read by a direct `mailbox` table lookup
/// — NOT the account-wide unread/total aggregation `list_mailboxes` runs — so
/// the fold's mailbox resolution stays inside the derive transaction and off
/// the single-writer critical section.
fn read_mailbox_id_by_role_on(
    connection: &Connection,
    account_id: &AccountId,
    role: &str,
) -> Result<Option<MailboxId>, StoreError> {
    connection
        .query_row(
            "SELECT id FROM mailbox WHERE account_id = ?1 AND role = ?2 LIMIT 1",
            params![account_id.as_str(), role],
            |row| row.get::<_, String>(0).map(MailboxId),
        )
        .optional()
        .map_err(sql_to_store_error)
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
