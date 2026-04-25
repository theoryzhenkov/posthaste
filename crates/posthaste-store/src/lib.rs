/// SQLite-backed `MailStore` implementation: sync batch writes, lazy body
/// fetching, conversation projections, smart mailbox queries, and event log.
///
/// @spec docs/L1-sync#sqlite-schema
mod db;
mod mutations;
mod projections;
mod query;
mod smart_mailboxes;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use hex::encode as hex_encode;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, synthesize_plain_text_raw_mime, AccountId,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationBackfillStore, CommandResult,
    ConversationCursor, ConversationId, ConversationPage, ConversationReadStore,
    ConversationSortField, ConversationSummary, ConversationView, DomainEvent, EventFilter,
    EventStore, FetchedBody, ImapMailboxSyncState, ImapModSeq, ImapSyncStateStore,
    ImapSyncStateWriteStore, ImapUid, ImapUidValidity, MailboxId, MailboxReadStore, MailboxSummary,
    MessageCommandStore, MessageCursor, MessageDetail, MessageDetailStore, MessageId,
    MessageListStore, MessageMailboxStore, MessagePage, MessageSortField, MessageSummary,
    RawMessageRef, ReplaceMailboxesCommand, SetKeywordsCommand, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxStore, SmartMailboxValue, SortDirection,
    SourceDataStore, SourceProjectionStore, StoreError, SyncBatch, SyncCursor, SyncObject,
    SyncStateStore, SyncWriteStore, TagReadStore, TagSummary, ThreadId, ThreadView,
    EVENT_TOPIC_MAILBOX_UPDATED, EVENT_TOPIC_MESSAGE_ARRIVED, EVENT_TOPIC_MESSAGE_UPDATED,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use tracing::{debug, info};

use crate::db::{
    bool_to_i64, configure_connection, init_schema, io_to_store_error, json_to_store_error,
    now_iso8601, parse_sync_object, sql_to_store_error,
};
use crate::mutations::{
    apply_message_body_tx, apply_sync_batch_tx, destroy_message_tx,
    list_events as list_events_for_filter, replace_mailboxes_tx, set_keywords_tx,
    stage_sync_bodies,
};
use crate::projections::{cleanup_orphan_conversations_tx, insert_event_tx, synthesize_raw_mime};
use crate::query::{
    fetch_mailbox_ids, fetch_message_attachments, hydrate_message_summaries,
    load_message_summary_rows, row_to_message_summary_row,
};
use crate::smart_mailboxes::{
    count_smart_mailbox_messages, query_conversations, query_conversations_by_rule,
    query_message_page, query_message_page_by_rule, query_messages_by_rule,
};

/// SQLite-backed store with a single serialized write connection and pooled
/// read connections. Raw MIME bodies are stored as content-addressed files
/// on disk.
///
/// @spec docs/L1-sync#sqlite-schema
/// @spec docs/L0-accounts#the-invariant
pub struct DatabaseStore {
    db_path: PathBuf,
    data_root: PathBuf,
    write_connection: Mutex<Connection>,
}

impl DatabaseStore {
    /// Opens (or creates) the SQLite database and data directory, runs schema
    /// migrations, and returns a ready-to-use store.
    pub fn open(
        db_path: impl Into<PathBuf>,
        data_root: impl Into<PathBuf>,
    ) -> Result<Self, StoreError> {
        let db_path = db_path.into();
        let data_root = data_root.into();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(io_to_store_error)?;
        }
        fs::create_dir_all(&data_root).map_err(io_to_store_error)?;

        let connection =
            Connection::open(&db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        configure_connection(&connection)?;
        init_schema(&connection)?;

        info!(db_path = %db_path.display(), "database store opened");
        Ok(Self {
            db_path,
            data_root,
            write_connection: Mutex::new(connection),
        })
    }

    /// Opens a new read-only SQLite connection (WAL mode allows concurrent
    /// readers).
    fn read_connection(&self) -> Result<Connection, StoreError> {
        let connection =
            Connection::open(&self.db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    /// Acquires the write lock and executes `operation` inside a single SQLite
    /// transaction. Rolls back on error.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    fn write_transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self
            .write_connection
            .lock()
            .map_err(|_| StoreError::Failure("write lock poisoned".to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        let result = operation(&tx)?;
        tx.commit()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        Ok(result)
    }

    /// Writes raw MIME to a content-addressed file under `data_root/accounts/
    /// {account_id}/messages/{sha256_prefix}/{sha256}.eml`. Deduplicates by
    /// hash.
    fn store_raw_message(
        &self,
        account_id: &AccountId,
        raw_mime: &str,
    ) -> Result<RawMessageRef, StoreError> {
        let mut hasher = Sha256::new();
        hasher.update(raw_mime.as_bytes());
        let sha256 = hex_encode(hasher.finalize());
        let prefix = &sha256[..2];
        let directory = self
            .data_root
            .join("accounts")
            .join(account_id.as_str())
            .join("messages")
            .join(prefix);
        fs::create_dir_all(&directory).map_err(io_to_store_error)?;
        let path = directory.join(format!("{sha256}.eml"));
        if !path.exists() {
            fs::write(&path, raw_mime).map_err(io_to_store_error)?;
        }
        Ok(RawMessageRef {
            path: path.to_string_lossy().to_string(),
            sha256,
            size: raw_mime.len() as i64,
            mime_type: "message/rfc822".to_string(),
            fetched_at: now_iso8601()?,
        })
    }

    /// Persists a sync state token in the same transaction as sync data.
    ///
    /// @spec docs/L1-sync#state-management
    fn upsert_sync_cursor_tx(
        tx: &Transaction<'_>,
        account_id: &AccountId,
        cursor: &SyncCursor,
    ) -> Result<(), StoreError> {
        tx.execute(
            "INSERT INTO sync_cursor (account_id, object_type, state, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, object_type) DO UPDATE SET
                state = excluded.state,
                updated_at = excluded.updated_at",
            params![
                account_id.as_str(),
                cursor.object_type.as_str(),
                cursor.state,
                cursor.updated_at
            ],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    }

    /// Lists all messages in a thread, ordered by `received_at ASC`.
    ///
    /// @spec docs/L1-search#thread-view
    fn list_messages_for_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM message m
                 JOIN source_projection a ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND m.thread_id = ?2
                 ORDER BY received_at ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), thread_id.as_str()],
        )?;
        hydrate_message_summaries(&connection, rows)
    }
}

impl SourceProjectionStore for DatabaseStore {
    /// Creates or updates the `source_projection` row that maps account IDs to
    /// display names for query joins.
    fn upsert_source_projection(
        &self,
        source_id: &AccountId,
        name: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO source_projection (source_id, name) VALUES (?1, ?2)
                 ON CONFLICT(source_id) DO UPDATE SET name = excluded.name",
                params![source_id.as_str(), name],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    /// Removes a source projection row when an account is deleted.
    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM source_projection WHERE source_id = ?1",
                params![source_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

impl MailboxReadStore for DatabaseStore {
    /// Lists mailboxes for an account, ordered by role then name.
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, role, unread_emails, total_emails
                 FROM mailbox
                 WHERE account_id = ?1
                 ORDER BY COALESCE(role, ''), name",
            )
            .map_err(sql_to_store_error)?;

        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(MailboxSummary {
                    id: MailboxId(row.get(0)?),
                    name: row.get(1)?,
                    role: row.get(2)?,
                    unread_emails: row.get(3)?,
                    total_emails: row.get(4)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let mailboxes = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(mailboxes)
    }
}

impl ConversationReadStore for DatabaseStore {
    /// Returns a seek-paginated page of conversations, optionally filtered by
    /// account and/or mailbox.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        let connection = self.read_connection()?;
        query_conversations(
            &connection,
            "WHERE (?1 IS NULL OR m.account_id = ?1)
               AND (
                 ?2 IS NULL OR EXISTS (
                     SELECT 1
                     FROM message_mailbox mm
                     WHERE mm.account_id = m.account_id
                       AND mm.message_id = m.id
                       AND mm.mailbox_id = ?2
                 )
               )",
            vec![
                account_id
                    .map(|source| SqlValue::Text(source.as_str().to_string()))
                    .unwrap_or(SqlValue::Null),
                mailbox_id
                    .map(|mailbox| SqlValue::Text(mailbox.as_str().to_string()))
                    .unwrap_or(SqlValue::Null),
            ],
            limit,
            cursor,
            sort_field,
            sort_direction,
        )
    }

    /// Returns all messages in a conversation ordered by `received_at ASC`,
    /// or `None` if the conversation does not exist.
    ///
    /// @spec docs/L1-search#conversation-view
    fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM conversation_message cm
                 JOIN message m
                   ON m.account_id = cm.account_id
                  AND m.id = cm.message_id
                 JOIN source_projection a
                   ON a.source_id = m.account_id
                 WHERE cm.conversation_id = ?1
                 ORDER BY m.received_at ASC, m.id ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(&mut statement, params![conversation_id.as_str()])?;
        let messages = hydrate_message_summaries(&connection, rows)?;
        if messages.is_empty() {
            return Ok(None);
        }
        let subject = messages
            .last()
            .and_then(|message| message.subject.clone())
            .or_else(|| messages.iter().find_map(|message| message.subject.clone()));
        Ok(Some(ConversationView {
            id: conversation_id.clone(),
            subject,
            messages,
        }))
    }
}

impl MessageListStore for DatabaseStore {
    /// Lists messages for an account, optionally filtered by mailbox, ordered
    /// by `received_at DESC`.
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let sql = if mailbox_id.is_some() {
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN source_projection a
               ON a.source_id = m.account_id
             JOIN message_mailbox mm
               ON mm.account_id = m.account_id
              AND mm.message_id = m.id
             WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
             ORDER BY m.received_at DESC"
        } else {
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1
             ORDER BY m.received_at DESC"
        };
        let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
        let summary_rows = if let Some(mailbox_id) = mailbox_id {
            load_message_summary_rows(
                &mut statement,
                params![account_id.as_str(), mailbox_id.as_str()],
            )?
        } else {
            load_message_summary_rows(&mut statement, params![account_id.as_str()])?
        };
        hydrate_message_summaries(&connection, summary_rows)
    }

    /// Returns a seek-paginated page of messages for an account, optionally
    /// filtered by mailbox.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn list_message_page(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        let connection = self.read_connection()?;
        query_message_page(
            &connection,
            "WHERE m.account_id = ?1
               AND (
                 ?2 IS NULL OR EXISTS (
                     SELECT 1
                     FROM message_mailbox mm
                     WHERE mm.account_id = m.account_id
                       AND mm.message_id = m.id
                       AND mm.mailbox_id = ?2
                 )
               )",
            vec![
                SqlValue::Text(account_id.as_str().to_string()),
                mailbox_id
                    .map(|mailbox| SqlValue::Text(mailbox.as_str().to_string()))
                    .unwrap_or(SqlValue::Null),
            ],
            limit,
            cursor,
            sort_field,
            sort_direction,
        )
    }
}

impl TagReadStore for DatabaseStore {
    fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT TRIM(mk.keyword) AS keyword,
                        COUNT(DISTINCT CASE WHEN m.is_read = 0 THEN m.id END) AS unread_messages,
                        COUNT(DISTINCT m.id) AS total_messages
                 FROM message_keyword mk
                 JOIN message m
                   ON m.account_id = mk.account_id
                  AND m.id = mk.message_id
                 WHERE mk.account_id = ?1
                   AND TRIM(mk.keyword) <> ''
                   AND TRIM(mk.keyword) NOT LIKE '$%'
                 GROUP BY TRIM(mk.keyword)
                 ORDER BY LOWER(TRIM(mk.keyword)), TRIM(mk.keyword)",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(TagSummary {
                    name: row.get(0)?,
                    unread_messages: row.get(1)?,
                    total_messages: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;

        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(sql_to_store_error)?);
        }
        Ok(tags)
    }
}

impl SmartMailboxStore for DatabaseStore {
    /// Evaluates a smart mailbox rule against all sources and returns matching
    /// messages.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        query_messages_by_rule(&connection, rule)
    }

    /// Evaluates a smart mailbox rule and returns a paginated message view.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        let connection = self.read_connection()?;
        query_message_page_by_rule(&connection, rule, limit, cursor, sort_field, sort_direction)
    }

    /// Evaluates a smart mailbox rule and returns a paginated conversation view.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        let connection = self.read_connection()?;
        query_conversations_by_rule(&connection, rule, limit, cursor, sort_field, sort_direction)
    }

    /// Returns (unread, total) message counts for a smart mailbox rule.
    fn query_smart_mailbox_counts(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), StoreError> {
        let connection = self.read_connection()?;
        count_smart_mailbox_messages(&connection, rule)
    }
}

fn parse_automation_backfill_status(
    value: String,
) -> Result<AutomationBackfillJobStatus, StoreError> {
    match value.as_str() {
        "pending" => Ok(AutomationBackfillJobStatus::Pending),
        "completed" => Ok(AutomationBackfillJobStatus::Completed),
        other => Err(StoreError::Failure(format!(
            "unknown automation backfill job status: {other}"
        ))),
    }
}

impl AutomationBackfillStore for DatabaseStore {
    fn ensure_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError> {
        self.write_transaction(|tx| {
            let timestamp = now_iso8601()?;
            tx.execute(
                "INSERT OR IGNORE INTO automation_backfill_job (
                    account_id, rule_fingerprint, status, attempts, last_error, queued_at, updated_at
                 )
                 VALUES (?1, ?2, 'pending', 0, NULL, ?3, ?3)",
                params![account_id.as_str(), rule_fingerprint, timestamp],
            )
            .map_err(sql_to_store_error)?;
            fetch_automation_backfill_job_tx(tx, account_id, rule_fingerprint)?
                .ok_or_else(|| StoreError::Failure("automation backfill job was not created".to_string()))
        })
    }

    fn complete_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE automation_backfill_job
                 SET status = 'completed', last_error = NULL, updated_at = ?3
                 WHERE account_id = ?1 AND rule_fingerprint = ?2",
                params![account_id.as_str(), rule_fingerprint, now_iso8601()?],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn record_automation_backfill_failure(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE automation_backfill_job
                 SET status = 'pending',
                     attempts = attempts + 1,
                     last_error = ?3,
                     updated_at = ?4
                 WHERE account_id = ?1 AND rule_fingerprint = ?2",
                params![account_id.as_str(), rule_fingerprint, error, now_iso8601()?],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn get_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<Option<AutomationBackfillJob>, StoreError> {
        let connection = self.read_connection()?;
        fetch_automation_backfill_job(&connection, account_id, rule_fingerprint)
    }
}

fn fetch_automation_backfill_job(
    connection: &Connection,
    account_id: &AccountId,
    rule_fingerprint: &str,
) -> Result<Option<AutomationBackfillJob>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT account_id, rule_fingerprint, status, attempts, last_error, updated_at
             FROM automation_backfill_job
             WHERE account_id = ?1 AND rule_fingerprint = ?2",
        )
        .map_err(sql_to_store_error)?;
    statement
        .query_row(params![account_id.as_str(), rule_fingerprint], |row| {
            let status: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                status,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .optional()
        .map_err(sql_to_store_error)?
        .map(
            |(account_id, rule_fingerprint, status, attempts, last_error, updated_at)| {
                Ok(AutomationBackfillJob {
                    account_id: AccountId::from(account_id),
                    rule_fingerprint,
                    status: parse_automation_backfill_status(status)?,
                    attempts,
                    last_error,
                    updated_at,
                })
            },
        )
        .transpose()
}

fn fetch_automation_backfill_job_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    rule_fingerprint: &str,
) -> Result<Option<AutomationBackfillJob>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT account_id, rule_fingerprint, status, attempts, last_error, updated_at
             FROM automation_backfill_job
             WHERE account_id = ?1 AND rule_fingerprint = ?2",
        )
        .map_err(sql_to_store_error)?;
    statement
        .query_row(params![account_id.as_str(), rule_fingerprint], |row| {
            let status: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                status,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .optional()
        .map_err(sql_to_store_error)?
        .map(
            |(account_id, rule_fingerprint, status, attempts, last_error, updated_at)| {
                Ok(AutomationBackfillJob {
                    account_id: AccountId::from(account_id),
                    rule_fingerprint,
                    status: parse_automation_backfill_status(status)?,
                    attempts,
                    last_error,
                    updated_at,
                })
            },
        )
        .transpose()
}

impl SourceDataStore for DatabaseStore {
    /// Removes all data for an account from every table, including orphaned
    /// conversations.
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM mailbox WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_mailbox WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_keyword WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_body WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_attachment WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM conversation_message WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM thread_view WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM sync_cursor WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM event_log WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM automation_backfill_job WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            cleanup_orphan_conversations_tx(tx)?;
            Ok(())
        })
    }
}

impl MessageDetailStore for DatabaseStore {
    /// Returns full message detail including body (if fetched) and raw message
    /// reference.
    ///
    /// @spec docs/L1-sync#email-bodies-are-fetched-lazily
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM message m
                 JOIN source_projection a
                   ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND m.id = ?2",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), message_id.as_str()],
        )?;
        let mut summaries = hydrate_message_summaries(&connection, rows)?;
        let Some(summary) = summaries.pop() else {
            return Ok(None);
        };

        let body = connection
            .query_row(
                "SELECT body_html, body_text, raw_path, raw_sha256, raw_size, raw_mime_type, fetched_at
                 FROM message_body
                 WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
                |row| {
                    let raw_path: Option<String> = row.get(2)?;
                    let raw_sha256: Option<String> = row.get(3)?;
                    let raw_size: Option<i64> = row.get(4)?;
                    let raw_mime_type: Option<String> = row.get(5)?;
                    let fetched_at: Option<String> = row.get(6)?;
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        raw_path.and_then(|path| {
                            Some(RawMessageRef {
                                path,
                                sha256: raw_sha256?,
                                size: raw_size?,
                                mime_type: raw_mime_type?,
                                fetched_at: fetched_at?,
                            })
                        }),
                    ))
                },
            )
            .optional()
            .map_err(sql_to_store_error)?;
        let attachments = fetch_message_attachments(&connection, account_id, message_id)?;

        Ok(Some(MessageDetail {
            summary,
            body_html: body.as_ref().and_then(|row| row.0.clone()),
            body_text: body.as_ref().and_then(|row| row.1.clone()),
            raw_message: body.and_then(|row| row.2),
            attachments,
        }))
    }

    /// Returns a thread view with all messages ordered chronologically, or
    /// `None` if empty.
    ///
    /// @spec docs/L1-search#thread-view
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError> {
        let messages = self.list_messages_for_thread(account_id, thread_id)?;
        if messages.is_empty() {
            return Ok(None);
        }
        Ok(Some(ThreadView {
            id: thread_id.clone(),
            messages,
        }))
    }
}

impl SyncStateStore for DatabaseStore {
    /// Returns all stored sync state tokens for an account.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT object_type, state, updated_at
                 FROM sync_cursor
                 WHERE account_id = ?1",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(SyncCursor {
                    object_type: parse_sync_object(&row.get::<_, String>(0)?)?,
                    state: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let cursors = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(cursors)
    }

    /// Returns the sync state token for a specific object type, or `None` if
    /// no sync has occurred yet.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError> {
        let connection = self.read_connection()?;
        connection
            .query_row(
                "SELECT state, updated_at
                 FROM sync_cursor
                 WHERE account_id = ?1 AND object_type = ?2",
                params![account_id.as_str(), object_type.as_str()],
                |row| {
                    Ok(SyncCursor {
                        object_type,
                        state: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(sql_to_store_error)
    }
}

impl ImapSyncStateStore for DatabaseStore {
    fn list_imap_mailbox_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT mailbox_id, mailbox_name, uid_validity, highest_uid,
                        highest_modseq, updated_at
                 FROM imap_mailbox_sync_state
                 WHERE account_id = ?1
                 ORDER BY mailbox_name, mailbox_id",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], imap_mailbox_state_from_row)
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    fn get_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError> {
        let connection = self.read_connection()?;
        connection
            .query_row(
                "SELECT mailbox_id, mailbox_name, uid_validity, highest_uid,
                        highest_modseq, updated_at
                 FROM imap_mailbox_sync_state
                 WHERE account_id = ?1 AND mailbox_id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
                imap_mailbox_state_from_row,
            )
            .optional()
            .map_err(sql_to_store_error)
    }
}

impl ImapSyncStateWriteStore for DatabaseStore {
    fn put_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        state: &ImapMailboxSyncState,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
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
                params![
                    account_id.as_str(),
                    state.mailbox_id.as_str(),
                    state.mailbox_name,
                    state.uid_validity.0,
                    state.highest_uid.map(|uid| uid.0),
                    state.highest_modseq.map(|modseq| modseq.0.to_string()),
                    state.updated_at,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn delete_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM imap_mailbox_sync_state
                 WHERE account_id = ?1 AND mailbox_id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

fn imap_mailbox_state_from_row(row: &Row<'_>) -> rusqlite::Result<ImapMailboxSyncState> {
    let uid_validity = u32_from_row(row, 2, "uid_validity")?;
    let highest_uid = optional_u32_from_row(row, 3, "highest_uid")?.map(ImapUid);
    let highest_modseq = optional_u64_text_from_row(row, 4, "highest_modseq")?.map(ImapModSeq);
    Ok(ImapMailboxSyncState {
        mailbox_id: MailboxId(row.get(0)?),
        mailbox_name: row.get(1)?,
        uid_validity: ImapUidValidity(uid_validity),
        highest_uid,
        highest_modseq,
        updated_at: row.get(5)?,
    })
}

fn optional_u32_from_row(
    row: &Row<'_>,
    index: usize,
    name: &'static str,
) -> rusqlite::Result<Option<u32>> {
    let Some(value) = row.get::<_, Option<i64>>(index)? else {
        return Ok(None);
    };
    u32::try_from(value).map(Some).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}

fn optional_u64_text_from_row(
    row: &Row<'_>,
    index: usize,
    name: &'static str,
) -> rusqlite::Result<Option<u64>> {
    let Some(value) = row.get::<_, Option<String>>(index)? else {
        return Ok(None);
    };
    value.parse::<u64>().map(Some).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}

fn u32_from_row(row: &Row<'_>, index: usize, name: &'static str) -> rusqlite::Result<u32> {
    let value = row.get::<_, i64>(index)?;
    u32::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}

impl MessageMailboxStore for DatabaseStore {
    /// Returns the mailbox IDs a message belongs to.
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        let connection = self.read_connection()?;
        fetch_mailbox_ids(&connection, account_id, message_id)
    }
}

impl SyncWriteStore for DatabaseStore {
    /// Applies a sync batch within a single SQLite transaction: stages raw
    /// bodies to disk first, then upserts/deletes mailboxes and messages,
    /// refreshes projections, and persists cursors atomically with data.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        debug!(
            account_id = %account_id,
            mailboxes = batch.mailboxes.len(),
            messages = batch.messages.len(),
            "applying sync batch to store"
        );
        let staged_bodies = stage_sync_bodies(self, account_id, batch)?;
        self.write_transaction(|tx| apply_sync_batch_tx(tx, account_id, batch, &staged_bodies))
    }

    /// Stores a lazily fetched message body and emits a `message.body_cached`
    /// event.
    ///
    /// @spec docs/L1-sync#invariants
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError> {
        let raw_ref = body
            .raw_mime
            .as_deref()
            .map(|raw_mime| self.store_raw_message(account_id, raw_mime))
            .transpose()?;
        self.write_transaction(|tx| {
            apply_message_body_tx(tx, account_id, message_id, body, raw_ref.as_ref())
        })
    }
}

impl MessageCommandStore for DatabaseStore {
    /// Adds/removes keywords on a message and refreshes mailbox counters.
    /// Optionally persists a new sync cursor atomically.
    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| set_keywords_tx(tx, account_id, message_id, cursor, command))
    }

    /// Replaces a message's mailbox memberships, refreshes counters, and emits
    /// arrival events for newly added mailboxes. Optionally persists a cursor.
    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| {
            replace_mailboxes_tx(tx, account_id, message_id, cursor, command)
        })
    }

    /// Permanently deletes a message and all its junction rows, refreshes
    /// thread/mailbox projections, and optionally persists a cursor.
    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| destroy_message_tx(tx, account_id, message_id, cursor))
    }
}

impl EventStore for DatabaseStore {
    /// Queries the event log, supporting `afterSeq` cursor-based replay.
    ///
    /// @spec docs/L1-sync#event-propagation
    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
        let connection = self.read_connection()?;
        list_events_for_filter(&connection, filter)
    }

    /// Inserts a domain event into the event log.
    ///
    /// @spec docs/L1-sync#event-propagation
    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: Value,
    ) -> Result<DomainEvent, StoreError> {
        self.write_transaction(|tx| {
            insert_event_tx(tx, account_id, topic, mailbox_id, message_id, payload)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use posthaste_domain::{
        search, MessageRecord, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
        SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
        SmartMailboxValue, SyncCursor,
    };

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("posthaste-store-test-{now}-{seq}"))
    }

    fn sample_message(
        message_id: &str,
        account_mailbox: &str,
        raw_mime: Option<&str>,
    ) -> MessageRecord {
        MessageRecord {
            id: MessageId::from(message_id),
            source_thread_id: ThreadId::from("thread-1"),
            remote_blob_id: None,
            subject: Some("Hello".to_string()),
            from_name: Some("Alice".to_string()),
            from_email: Some("alice@example.com".to_string()),
            preview: Some("Preview".to_string()),
            received_at: "2026-03-31T10:00:00Z".to_string(),
            has_attachment: false,
            size: 42,
            mailbox_ids: vec![MailboxId::from(account_mailbox)],
            keywords: vec!["$seen".to_string()],
            body_html: Some("<p>Hello</p>".to_string()),
            body_text: Some("Hello".to_string()),
            raw_mime: raw_mime.map(str::to_string),
            rfc_message_id: Some(format!("<{message_id}@example.test>")),
            in_reply_to: None,
            references: Vec::new(),
        }
    }

    fn setup_source(
        store: &DatabaseStore,
        account_id: &AccountId,
        name: &str,
    ) -> Result<(), StoreError> {
        store.upsert_source_projection(account_id, name)
    }

    fn message_cursor(state: &str, updated_at: &str) -> SyncCursor {
        SyncCursor {
            object_type: SyncObject::Message,
            state: state.to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    fn seed_messages(
        store: &DatabaseStore,
        account_id: &AccountId,
        messages: Vec<MessageRecord>,
        cursor_state: &str,
    ) -> Result<(), StoreError> {
        store.apply_sync_batch(
            account_id,
            &SyncBatch {
                mailboxes: vec![
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("archive"),
                        name: "Archive".to_string(),
                        role: Some("archive".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                ],
                messages,
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![message_cursor(cursor_state, "2026-03-31T10:00:00Z")],
            },
        )?;
        Ok(())
    }

    #[test]
    fn imap_mailbox_state_round_trips_provider_cursors() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        let mut state = ImapMailboxSyncState::new(
            MailboxId::from("imap:inbox"),
            "INBOX".to_string(),
            ImapUidValidity(u32::MAX),
            "2026-04-25T00:00:00Z".to_string(),
        );
        state.record_seen_uid(ImapUid(u32::MAX));
        state.record_highest_modseq(ImapModSeq(u64::MAX));

        store.put_imap_mailbox_state(&account, &state)?;

        let loaded = store
            .get_imap_mailbox_state(&account, &MailboxId::from("imap:inbox"))?
            .expect("stored state");
        assert_eq!(loaded, state);
        assert_eq!(store.list_imap_mailbox_states(&account)?, vec![state]);
        Ok(())
    }

    #[test]
    fn imap_mailbox_state_delete_is_account_scoped() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let primary = AccountId::from("primary");
        let secondary = AccountId::from("secondary");
        let state = ImapMailboxSyncState::new(
            MailboxId::from("imap:inbox"),
            "INBOX".to_string(),
            ImapUidValidity(1),
            "2026-04-25T00:00:00Z".to_string(),
        );

        store.put_imap_mailbox_state(&primary, &state)?;
        store.put_imap_mailbox_state(&secondary, &state)?;
        store.delete_imap_mailbox_state(&primary, &MailboxId::from("imap:inbox"))?;

        assert!(store
            .get_imap_mailbox_state(&primary, &MailboxId::from("imap:inbox"))?
            .is_none());
        assert_eq!(
            store.get_imap_mailbox_state(&secondary, &MailboxId::from("imap:inbox"))?,
            Some(state)
        );
        Ok(())
    }

    fn rule_condition(
        field: SmartMailboxField,
        operator: SmartMailboxOperator,
        value: impl Into<String>,
    ) -> SmartMailboxRuleNode {
        SmartMailboxRuleNode::Condition(SmartMailboxCondition {
            field,
            operator,
            negated: false,
            value: SmartMailboxValue::String(value.into()),
        })
    }

    fn all_rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
        SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes,
            },
        }
    }

    #[test]
    fn message_page_sorts_and_paginates() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![
                MessageRecord {
                    id: MessageId::from("message-c"),
                    subject: Some("Charlie".to_string()),
                    received_at: "2026-04-03T10:00:00Z".to_string(),
                    ..sample_message("message-c", "inbox", Some("mime-c"))
                },
                MessageRecord {
                    id: MessageId::from("message-a"),
                    subject: Some("Alpha".to_string()),
                    received_at: "2026-04-01T10:00:00Z".to_string(),
                    ..sample_message("message-a", "inbox", Some("mime-a"))
                },
                MessageRecord {
                    id: MessageId::from("message-b"),
                    subject: Some("Bravo".to_string()),
                    received_at: "2026-04-02T10:00:00Z".to_string(),
                    ..sample_message("message-b", "inbox", Some("mime-b"))
                },
            ],
            "state",
        )?;

        let first_page = store.list_message_page(
            &account,
            None,
            2,
            None,
            MessageSortField::Subject,
            SortDirection::Asc,
        )?;
        assert_eq!(
            first_page
                .items
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["message-a", "message-b"]
        );
        let cursor = first_page
            .next_cursor
            .as_ref()
            .expect("first page should expose a next cursor");

        let second_page = store.list_message_page(
            &account,
            None,
            2,
            Some(cursor),
            MessageSortField::Subject,
            SortDirection::Asc,
        )?;
        assert_eq!(
            second_page
                .items
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["message-c"]
        );
        assert!(second_page.next_cursor.is_none());
        Ok(())
    }

    #[test]
    fn message_page_paginates_empty_sort_values() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![
                MessageRecord {
                    id: MessageId::from("blank-subject"),
                    subject: None,
                    ..sample_message("blank-subject", "inbox", Some("mime-blank"))
                },
                MessageRecord {
                    id: MessageId::from("alpha-subject"),
                    subject: Some("Alpha".to_string()),
                    ..sample_message("alpha-subject", "inbox", Some("mime-alpha"))
                },
            ],
            "state",
        )?;

        let first_page = store.list_message_page(
            &account,
            None,
            1,
            None,
            MessageSortField::Subject,
            SortDirection::Asc,
        )?;
        assert_eq!(first_page.items[0].id.as_str(), "blank-subject");
        assert_eq!(
            first_page
                .next_cursor
                .as_ref()
                .expect("first page should expose a next cursor")
                .sort_value,
            ""
        );

        let second_page = store.list_message_page(
            &account,
            None,
            1,
            first_page.next_cursor.as_ref(),
            MessageSortField::Subject,
            SortDirection::Asc,
        )?;
        assert_eq!(second_page.items[0].id.as_str(), "alpha-subject");
        assert!(second_page.next_cursor.is_none());
        Ok(())
    }

    #[test]
    fn message_page_rule_query_filters_source_mailbox_and_text() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let primary = AccountId::from("primary");
        let secondary = AccountId::from("secondary");
        setup_source(&store, &primary, "Primary")?;
        setup_source(&store, &secondary, "Secondary")?;
        seed_messages(
            &store,
            &primary,
            vec![
                MessageRecord {
                    id: MessageId::from("match"),
                    subject: Some("Posthaste account created".to_string()),
                    mailbox_ids: vec![MailboxId::from("inbox")],
                    ..sample_message("match", "inbox", Some("mime-match"))
                },
                MessageRecord {
                    id: MessageId::from("wrong-mailbox"),
                    subject: Some("Posthaste account created".to_string()),
                    mailbox_ids: vec![MailboxId::from("archive")],
                    ..sample_message("wrong-mailbox", "archive", Some("mime-archive"))
                },
            ],
            "primary-state",
        )?;
        seed_messages(
            &store,
            &secondary,
            vec![MessageRecord {
                id: MessageId::from("wrong-source"),
                subject: Some("Posthaste account created".to_string()),
                mailbox_ids: vec![MailboxId::from("inbox")],
                ..sample_message("wrong-source", "inbox", Some("mime-source"))
            }],
            "secondary-state",
        )?;

        let page = store.query_message_page_by_rule(
            &all_rule(vec![
                rule_condition(
                    SmartMailboxField::SourceId,
                    SmartMailboxOperator::Equals,
                    "primary",
                ),
                rule_condition(
                    SmartMailboxField::MailboxId,
                    SmartMailboxOperator::Equals,
                    "inbox",
                ),
                rule_condition(
                    SmartMailboxField::Subject,
                    SmartMailboxOperator::Contains,
                    "Posthaste",
                ),
            ]),
            10,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )?;

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id.as_str(), "match");
        assert_eq!(page.items[0].source_id, primary);
        assert_eq!(page.items[0].mailbox_ids, vec![MailboxId::from("inbox")]);
        assert!(page.next_cursor.is_none());
        Ok(())
    }

    #[test]
    fn parsed_message_query_executes_richer_filters() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary Account")?;
        seed_messages(
            &store,
            &account,
            vec![
                MessageRecord {
                    id: MessageId::from("match"),
                    source_thread_id: ThreadId::from("thread-match"),
                    subject: Some("Posthaste account created".to_string()),
                    mailbox_ids: vec![MailboxId::from("archive")],
                    keywords: Vec::new(),
                    ..sample_message("match", "archive", Some("mime-match"))
                },
                MessageRecord {
                    id: MessageId::from("read-message"),
                    source_thread_id: ThreadId::from("thread-match"),
                    subject: Some("Posthaste account created".to_string()),
                    mailbox_ids: vec![MailboxId::from("archive")],
                    keywords: vec!["$seen".to_string()],
                    ..sample_message("read-message", "archive", Some("mime-read"))
                },
            ],
            "state",
        )?;

        let rule = search::parse_query(
            "source: Primary Account in:Archive is:unread subject:account created id:match thread:thread-match",
        )
        .map_err(StoreError::Failure)?;
        let page = store.query_message_page_by_rule(
            &rule,
            10,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )?;

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id.as_str(), "match");
        assert!(!page.items[0].is_read);
        Ok(())
    }

    #[test]
    fn list_tags_returns_user_keywords_with_counts() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![
                MessageRecord {
                    id: MessageId::from("read-newsletter"),
                    keywords: vec!["$seen".to_string(), "newsletter".to_string()],
                    ..sample_message("read-newsletter", "inbox", Some("mime-read-newsletter"))
                },
                MessageRecord {
                    id: MessageId::from("unread-newsletter"),
                    keywords: vec![
                        "newsletter".to_string(),
                        "work".to_string(),
                        "".to_string(),
                        "   ".to_string(),
                        "$custom".to_string(),
                    ],
                    ..sample_message("unread-newsletter", "inbox", Some("mime-unread-newsletter"))
                },
            ],
            "state",
        )?;

        let tags = store.list_tags(&account)?;

        assert_eq!(
            tags,
            vec![
                TagSummary {
                    name: "newsletter".to_string(),
                    unread_messages: 1,
                    total_messages: 2,
                },
                TagSummary {
                    name: "work".to_string(),
                    unread_messages: 1,
                    total_messages: 1,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn account_scoped_reads_do_not_leak() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");
        setup_source(&store, &account_a, "Primary")?;
        setup_source(&store, &account_b, "Secondary")?;

        store.apply_sync_batch(
            &account_a,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message("shared-id", "inbox", Some("mime-a"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "a".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )?;
        store.apply_sync_batch(
            &account_b,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message("shared-id", "inbox", Some("mime-b"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "b".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )?;

        let detail_a = store
            .get_message_detail(&account_a, &MessageId::from("shared-id"))?
            .unwrap();
        let detail_b = store
            .get_message_detail(&account_b, &MessageId::from("shared-id"))?
            .unwrap();
        assert_ne!(
            detail_a.raw_message.as_ref().unwrap().path,
            detail_b.raw_message.as_ref().unwrap().path
        );
        Ok(())
    }

    #[test]
    fn sync_batch_is_atomic_when_junction_insert_fails() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        let result = store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![MessageRecord {
                    mailbox_ids: vec![MailboxId::from("inbox"), MailboxId::from("inbox")],
                    ..sample_message("message-1", "inbox", Some("mime"))
                }],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "state".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        );
        assert!(result.is_err());
        assert!(store.list_messages(&account, None)?.is_empty());
        assert!(store.get_cursor(&account, SyncObject::Message)?.is_none());
        Ok(())
    }

    #[test]
    fn event_replay_respects_after_seq() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");

        let first = store.append_event(
            &account,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            None,
            json!({"n": 1}),
        )?;
        let _second = store.append_event(
            &account,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            None,
            json!({"n": 2}),
        )?;

        let events = store.list_events(&EventFilter {
            account_id: Some(account),
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
            mailbox_id: None,
            after_seq: Some(first.seq),
        })?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["n"], 2);
        Ok(())
    }

    #[test]
    fn event_replay_compares_after_seq_as_integer() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");

        for n in 1..=11 {
            store.append_event(
                &account,
                EVENT_TOPIC_MESSAGE_UPDATED,
                None,
                None,
                json!({ "n": n }),
            )?;
        }

        let events = store.list_events(&EventFilter {
            account_id: Some(account),
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
            mailbox_id: None,
            after_seq: Some(9),
        })?;

        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![10, 11]
        );
        assert_eq!(
            events
                .iter()
                .map(|event| event.payload["n"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
        Ok(())
    }

    #[test]
    fn smart_mailbox_queries_messages_across_enabled_sources() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");
        setup_source(&store, &account_a, "Primary")?;
        setup_source(&store, &account_b, "Secondary")?;

        for account in [&account_a, &account_b] {
            store.apply_sync_batch(
                account,
                &SyncBatch {
                    mailboxes: vec![posthaste_domain::MailboxRecord {
                        id: MailboxId::from("inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    }],
                    messages: vec![sample_message(
                        &format!("message-{}", account.as_str()),
                        "inbox",
                        Some("mime"),
                    )],
                    deleted_mailbox_ids: Vec::new(),
                    deleted_message_ids: Vec::new(),
                    replace_all_mailboxes: false,
                    replace_all_messages: false,
                    cursors: vec![SyncCursor {
                        object_type: SyncObject::Message,
                        state: "state".to_string(),
                        updated_at: "2026-03-31T10:00:00Z".to_string(),
                    }],
                },
            )?;
        }

        let rule = SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::MailboxRole,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("inbox".to_string()),
                })],
            },
        };

        let messages = store.query_messages_by_rule(&rule)?;

        assert_eq!(messages.len(), 2);
        assert!(messages
            .iter()
            .any(|message| message.source_id == account_a));
        assert!(messages
            .iter()
            .any(|message| message.source_id == account_b));
        Ok(())
    }

    #[test]
    fn bulk_message_hydration_preserves_order_and_account_scoped_metadata() -> Result<(), StoreError>
    {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");
        setup_source(&store, &account_a, "Primary")?;
        setup_source(&store, &account_b, "Secondary")?;

        store.apply_sync_batch(
            &account_a,
            &SyncBatch {
                mailboxes: vec![
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("archive"),
                        name: "Archive".to_string(),
                        role: Some("archive".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                ],
                messages: vec![
                    MessageRecord {
                        received_at: "2026-03-31T11:00:00Z".to_string(),
                        mailbox_ids: vec![MailboxId::from("inbox")],
                        keywords: vec!["$flagged".to_string(), "zeta".to_string()],
                        ..sample_message("newer", "inbox", Some("mime-newer"))
                    },
                    MessageRecord {
                        received_at: "2026-03-31T10:00:00Z".to_string(),
                        mailbox_ids: vec![MailboxId::from("archive"), MailboxId::from("inbox")],
                        keywords: vec!["$seen".to_string(), "alpha".to_string()],
                        ..sample_message("shared-id", "inbox", Some("mime-a"))
                    },
                ],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "state-a".to_string(),
                    updated_at: "2026-03-31T11:00:00Z".to_string(),
                }],
            },
        )?;

        store.apply_sync_batch(
            &account_b,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("trash"),
                    name: "Trash".to_string(),
                    role: Some("trash".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![MessageRecord {
                    mailbox_ids: vec![MailboxId::from("trash")],
                    keywords: vec!["beta".to_string()],
                    ..sample_message("shared-id", "trash", Some("mime-b"))
                }],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "state-b".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )?;

        let listed = store.list_messages(&account_a, None)?;
        assert_eq!(
            listed
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["newer", "shared-id"]
        );
        assert_eq!(listed[0].mailbox_ids, vec![MailboxId::from("inbox")]);
        assert_eq!(
            listed[0].keywords,
            vec!["$flagged".to_string(), "zeta".to_string()]
        );
        assert_eq!(
            listed[1].mailbox_ids,
            vec![MailboxId::from("archive"), MailboxId::from("inbox")]
        );
        assert_eq!(
            listed[1].keywords,
            vec!["$seen".to_string(), "alpha".to_string()]
        );

        let queried = store.query_messages_by_rule(&SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::Keyword,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("beta".to_string()),
                })],
            },
        })?;
        assert_eq!(queried.len(), 1);
        assert_eq!(queried[0].source_id, account_b);
        assert_eq!(queried[0].mailbox_ids, vec![MailboxId::from("trash")]);
        assert_eq!(queried[0].keywords, vec!["beta".to_string()]);
        Ok(())
    }

    #[test]
    fn list_conversations_preserves_source_names_with_commas() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary, Inc.")?;

        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message("message-1", "inbox", Some("mime"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "state".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )?;

        let page = store.list_conversations(
            Some(&account),
            None,
            10,
            None,
            ConversationSortField::default(),
            SortDirection::default(),
        )?;

        assert_eq!(page.items.len(), 1);
        assert_eq!(
            page.items[0].source_names,
            vec!["Primary, Inc.".to_string()]
        );
        assert_eq!(page.items[0].latest_source_name, "Primary, Inc.");
        Ok(())
    }

    #[test]
    fn conversations_follow_jmap_thread_id_not_headers_or_subject() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;

        let first = sample_message("message-1", "inbox", Some("mime-1"));
        let mut second = sample_message("message-2", "inbox", Some("mime-2"));
        second.source_thread_id = ThreadId::from("thread-2");
        second.subject = first.subject.clone();
        second.in_reply_to = first.rfc_message_id.clone();
        second.references = first.rfc_message_id.iter().cloned().collect();

        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![first, second],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: true,
                replace_all_messages: true,
                cursors: vec![message_cursor("message-1", "2026-03-31T10:00:00Z")],
            },
        )?;

        let page = store.list_conversations(
            Some(&account),
            None,
            10,
            None,
            ConversationSortField::default(),
            SortDirection::default(),
        )?;

        assert_eq!(page.items.len(), 2);
        Ok(())
    }

    #[test]
    fn arrival_event_only_emits_for_new_mailbox_membership() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        let first_batch = SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("archive"),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
            ],
            messages: vec![sample_message("message-1", "inbox", Some("mime"))],
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state-1".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        };
        let second_batch = SyncBatch {
            mailboxes: first_batch.mailboxes.clone(),
            messages: vec![MessageRecord {
                mailbox_ids: vec![MailboxId::from("archive"), MailboxId::from("inbox")],
                ..sample_message("message-1", "inbox", Some("mime"))
            }],
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state-2".to_string(),
                updated_at: "2026-03-31T10:05:00Z".to_string(),
            }],
        };

        let first_events = store.apply_sync_batch(&account, &first_batch)?;
        let second_events = store.apply_sync_batch(&account, &second_batch)?;

        let first_arrivals: Vec<_> = first_events
            .iter()
            .filter(|event| event.topic == EVENT_TOPIC_MESSAGE_ARRIVED)
            .collect();
        let second_arrivals: Vec<_> = second_events
            .iter()
            .filter(|event| event.topic == EVENT_TOPIC_MESSAGE_ARRIVED)
            .collect();

        assert_eq!(first_arrivals.len(), 1);
        assert_eq!(
            first_arrivals[0].mailbox_id.as_ref().map(MailboxId::as_str),
            Some("inbox")
        );
        assert_eq!(second_arrivals.len(), 1);
        assert_eq!(
            second_arrivals[0]
                .mailbox_id
                .as_ref()
                .map(MailboxId::as_str),
            Some("archive")
        );
        Ok(())
    }

    #[test]
    fn raw_message_store_deduplicates_by_hash() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        let first = store.store_raw_message(&account, "same mime")?;
        let second = store.store_raw_message(&account, "same mime")?;
        assert_eq!(first.path, second.path);
        assert_eq!(first.sha256, second.sha256);
        Ok(())
    }

    #[test]
    fn set_keywords_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![sample_message("message-1", "inbox", Some("mime"))],
            "message-1",
        )?;

        store.set_keywords(
            &account,
            &MessageId::from("message-1"),
            Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );

        store.set_keywords(
            &account,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: Vec::new(),
                remove: vec!["$flagged".to_string()],
            },
        )?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );
        Ok(())
    }

    #[test]
    fn replace_mailboxes_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError>
    {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![sample_message("message-1", "inbox", Some("mime"))],
            "message-1",
        )?;

        store.replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );

        store.replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            None,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("inbox")],
            },
        )?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );
        Ok(())
    }

    #[test]
    fn destroy_message_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![
                sample_message("message-1", "inbox", Some("mime-1")),
                sample_message("message-2", "inbox", Some("mime-2")),
            ],
            "message-1",
        )?;

        store.destroy_message(
            &account,
            &MessageId::from("message-1"),
            Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
        )?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );

        store.destroy_message(&account, &MessageId::from("message-2"), None)?;
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)?
                .expect("cursor should exist")
                .state,
            "message-2"
        );
        Ok(())
    }

    #[test]
    fn full_mailbox_snapshot_removes_stale_local_mailboxes() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;

        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                    posthaste_domain::MailboxRecord {
                        id: MailboxId::from("all-mail"),
                        name: "All Mail".to_string(),
                        role: None,
                        unread_emails: 0,
                        total_emails: 0,
                    },
                ],
                messages: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: true,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: "mailbox-1".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )?;

        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![posthaste_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: true,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: "mailbox-2".to_string(),
                    updated_at: "2026-03-31T10:05:00Z".to_string(),
                }],
            },
        )?;

        let mailboxes = store.list_mailboxes(&account)?;
        assert_eq!(mailboxes.len(), 1);
        assert_eq!(mailboxes[0].id, MailboxId::from("inbox"));
        Ok(())
    }

    #[test]
    fn full_message_snapshot_removes_stale_local_messages() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary")?;

        let mailbox = posthaste_domain::MailboxRecord {
            id: MailboxId::from("inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 0,
            total_emails: 0,
        };
        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![mailbox.clone()],
                messages: vec![
                    sample_message("message-1", "inbox", Some("mime-1")),
                    sample_message("message-2", "inbox", Some("mime-2")),
                ],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: true,
                replace_all_messages: true,
                cursors: vec![message_cursor("message-1", "2026-03-31T10:00:00Z")],
            },
        )?;

        store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![mailbox],
                messages: vec![sample_message("message-2", "inbox", Some("mime-2"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: true,
                cursors: vec![message_cursor("message-2", "2026-03-31T10:05:00Z")],
            },
        )?;

        let messages = store.list_messages(&account, None)?;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, MessageId::from("message-2"));
        assert!(store
            .get_message_detail(&account, &MessageId::from("message-1"))?
            .is_none());
        Ok(())
    }
}
