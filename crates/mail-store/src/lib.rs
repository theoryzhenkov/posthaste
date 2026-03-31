use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use hex::encode as hex_encode;
use mail_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    CommandResult, ConversationId, ConversationSummary, ConversationView, DomainEvent,
    EventFilter, FetchedBody, MailStore, MailboxId, MailboxSummary, MessageDetail, MessageId,
    MessageSummary, RawMessageRef, ReplaceMailboxesCommand, SetKeywordsCommand, SidebarResponse,
    SidebarSmartMailbox, SidebarSource, SmartMailbox, SmartMailboxCondition, SmartMailboxField,
    SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxId, SmartMailboxKind,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxSummary,
    SmartMailboxValue, StoreError, SyncBatch, SyncCursor, SyncObject, ThreadId, ThreadView,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct DatabaseStore {
    db_path: PathBuf,
    data_root: PathBuf,
    write_connection: Mutex<Connection>,
}

impl DatabaseStore {
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
        seed_default_smart_mailboxes(&connection)?;

        Ok(Self {
            db_path,
            data_root,
            write_connection: Mutex::new(connection),
        })
    }

    fn read_connection(&self) -> Result<Connection, StoreError> {
        let connection =
            Connection::open(&self.db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        configure_connection(&connection)?;
        Ok(connection)
    }

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
                 JOIN account_config a ON a.account_id = m.account_id
                 WHERE m.account_id = ?1 AND m.thread_id = ?2
                 ORDER BY received_at ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str(), thread_id.as_str()], |row| {
                row_to_message_summary(&connection, row)
            })
            .map_err(sql_to_store_error)?;
        let messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(messages)
    }
}

#[async_trait]
impl MailStore for DatabaseStore {
    fn get_app_settings(&self) -> Result<AppSettings, StoreError> {
        let connection = self.read_connection()?;
        let json: Option<String> = connection
            .query_row(
                "SELECT settings_json FROM app_settings WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        match json {
            Some(json) => serde_json::from_str(&json).map_err(json_to_store_error),
            None => Ok(AppSettings::default()),
        }
    }

    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO app_settings (singleton, settings_json)
                 VALUES (1, ?1)
                 ON CONFLICT(singleton) DO UPDATE SET settings_json = excluded.settings_json",
                params![serde_json::to_string(settings).map_err(json_to_store_error)?],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn list_accounts(&self) -> Result<Vec<AccountSettings>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, name, driver, enabled, transport_json, created_at, updated_at
                 FROM account_config
                 ORDER BY name, account_id",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map([], row_to_account_settings)
            .map_err(sql_to_store_error)?;
        let accounts = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(accounts)
    }

    fn get_account(&self, account_id: &AccountId) -> Result<Option<AccountSettings>, StoreError> {
        let connection = self.read_connection()?;
        connection
            .query_row(
                "SELECT account_id, name, driver, enabled, transport_json, created_at, updated_at
                 FROM account_config
                 WHERE account_id = ?1",
                params![account_id.as_str()],
                row_to_account_settings,
            )
            .optional()
            .map_err(sql_to_store_error)
    }

    fn create_account(&self, account: &AccountSettings) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            let changed = tx
                .execute(
                    "INSERT INTO account_config (
                        account_id, name, driver, enabled, transport_json, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        account.id.as_str(),
                        account.name,
                        account.driver.as_str(),
                        bool_to_i64(account.enabled),
                        serde_json::to_string(&account.transport).map_err(json_to_store_error)?,
                        account.created_at,
                        account.updated_at,
                    ],
                )
                .map_err(sql_to_store_error)?;
            if changed != 1 {
                return Err(StoreError::Conflict(format!(
                    "account:{}",
                    account.id.as_str()
                )));
            }
            Ok(())
        })
    }

    fn update_account(&self, account: &AccountSettings) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            let changed = tx
                .execute(
                    "UPDATE account_config
                     SET name = ?2,
                         driver = ?3,
                         enabled = ?4,
                         transport_json = ?5,
                         updated_at = ?6
                     WHERE account_id = ?1",
                    params![
                        account.id.as_str(),
                        account.name,
                        account.driver.as_str(),
                        bool_to_i64(account.enabled),
                        serde_json::to_string(&account.transport).map_err(json_to_store_error)?,
                        account.updated_at,
                    ],
                )
                .map_err(sql_to_store_error)?;
            if changed == 0 {
                return Err(StoreError::NotFound(format!(
                    "account:{}",
                    account.id.as_str()
                )));
            }
            Ok(())
        })
    }

    fn delete_account(&self, account_id: &AccountId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM app_settings
                 WHERE singleton = 1
                   AND json_extract(settings_json, '$.defaultAccountId') = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM account_config WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
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
            cleanup_orphan_conversations_tx(tx)?;
            Ok(())
        })
    }

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

    fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mailbox_filter = mailbox_id.map(|mailbox| mailbox.as_str().to_string());
        let source_filter = account_id.map(|source| source.as_str().to_string());
        let mut statement = connection
            .prepare(
                "WITH filtered AS (
                    SELECT
                        m.conversation_id,
                        m.account_id,
                        a.name AS account_name,
                        m.id,
                        m.subject,
                        m.from_name,
                        m.from_email,
                        m.preview,
                        m.received_at,
                        m.has_attachment,
                        m.is_read,
                        m.is_flagged
                    FROM message m
                    JOIN account_config a
                      ON a.account_id = m.account_id
                    WHERE a.enabled = 1
                      AND (?1 IS NULL OR m.account_id = ?1)
                      AND (
                        ?2 IS NULL OR EXISTS (
                            SELECT 1
                            FROM message_mailbox mm
                            WHERE mm.account_id = m.account_id
                              AND mm.message_id = m.id
                              AND mm.mailbox_id = ?2
                        )
                      )
                ),
                ranked AS (
                    SELECT
                        filtered.*,
                        ROW_NUMBER() OVER (
                            PARTITION BY filtered.conversation_id
                            ORDER BY filtered.received_at DESC, filtered.id DESC
                        ) AS row_number,
                        COUNT(*) OVER (PARTITION BY filtered.conversation_id) AS message_count,
                        SUM(CASE WHEN filtered.is_read = 0 THEN 1 ELSE 0 END)
                            OVER (PARTITION BY filtered.conversation_id) AS unread_count
                    FROM filtered
                ),
                source_groups AS (
                    SELECT
                        filtered.conversation_id,
                        GROUP_CONCAT(DISTINCT filtered.account_id) AS source_ids,
                        GROUP_CONCAT(DISTINCT filtered.account_name) AS source_names
                    FROM filtered
                    GROUP BY filtered.conversation_id
                )
                SELECT
                    ranked.conversation_id,
                    ranked.subject,
                    ranked.preview,
                    ranked.from_name,
                    ranked.from_email,
                    ranked.received_at,
                    ranked.unread_count,
                    ranked.message_count,
                    source_groups.source_ids,
                    source_groups.source_names,
                    ranked.account_id,
                    ranked.account_name,
                    ranked.id,
                    ranked.has_attachment,
                    ranked.is_flagged
                FROM ranked
                JOIN source_groups
                  ON source_groups.conversation_id = ranked.conversation_id
                WHERE ranked.row_number = 1
                ORDER BY ranked.received_at DESC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![source_filter, mailbox_filter], |row| {
                Ok(ConversationSummary {
                    id: ConversationId(row.get(0)?),
                    subject: row.get(1)?,
                    preview: row.get(2)?,
                    from_name: row.get(3)?,
                    from_email: row.get(4)?,
                    latest_received_at: row.get(5)?,
                    unread_count: row.get(6)?,
                    message_count: row.get(7)?,
                    source_ids: split_group_concat_ids(row.get::<_, Option<String>>(8)?),
                    source_names: split_group_concat_strings(row.get::<_, Option<String>>(9)?),
                    latest_message: mail_domain::SourceMessageRef {
                        source_id: AccountId(row.get(10)?),
                        message_id: MessageId(row.get(12)?),
                    },
                    latest_source_name: row.get(11)?,
                    has_attachment: row.get::<_, i64>(13)? != 0,
                    is_flagged: row.get::<_, i64>(14)? != 0,
                })
            })
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_to_store_error)
    }

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
                 JOIN account_config a
                   ON a.account_id = m.account_id
                 WHERE cm.conversation_id = ?1
                 ORDER BY m.received_at ASC, m.id ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![conversation_id.as_str()], |row| {
                row_to_message_summary(&connection, row)
            })
            .map_err(sql_to_store_error)?;
        let messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
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

    fn get_sidebar(&self) -> Result<SidebarResponse, StoreError> {
        let accounts = self.list_accounts()?;
        let smart_mailboxes = self
            .list_smart_mailboxes()?
            .into_iter()
            .map(|mailbox| SidebarSmartMailbox {
                id: mailbox.id,
                name: mailbox.name,
                unread_messages: mailbox.unread_messages,
                total_messages: mailbox.total_messages,
            })
            .collect();
        let mut sources = Vec::new();
        for account in accounts.into_iter().filter(|account| account.enabled) {
            sources.push(SidebarSource {
                id: account.id.clone(),
                name: account.name,
                mailboxes: self.list_mailboxes(&account.id)?,
            });
        }
        Ok(SidebarResponse {
            smart_mailboxes,
            sources,
        })
    }

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
             JOIN account_config a
               ON a.account_id = m.account_id
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
             JOIN account_config a
               ON a.account_id = m.account_id
             WHERE m.account_id = ?1
             ORDER BY m.received_at DESC"
        };
        let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
        let summaries = if let Some(mailbox_id) = mailbox_id {
            statement
                .query_map(params![account_id.as_str(), mailbox_id.as_str()], |row| {
                    row_to_message_summary(&connection, row)
                })
                .map_err(sql_to_store_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_to_store_error)?
        } else {
            statement
                .query_map(params![account_id.as_str()], |row| {
                    row_to_message_summary(&connection, row)
                })
                .map_err(sql_to_store_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_to_store_error)?
        };
        Ok(summaries)
    }

    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mailboxes = load_smart_mailboxes(&connection)?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            let (total_messages, unread_messages) =
                count_smart_mailbox_messages(&connection, &mailbox.rule)?;
            summaries.push(SmartMailboxSummary {
                id: mailbox.id,
                name: mailbox.name,
                position: mailbox.position,
                kind: mailbox.kind,
                default_key: mailbox.default_key,
                parent_id: mailbox.parent_id,
                unread_messages,
                total_messages,
                created_at: mailbox.created_at,
                updated_at: mailbox.updated_at,
            });
        }
        Ok(summaries)
    }

    fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Option<SmartMailbox>, StoreError> {
        let connection = self.read_connection()?;
        query_smart_mailbox(&connection, smart_mailbox_id)
    }

    fn create_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), StoreError> {
        validate_smart_mailbox(smart_mailbox)?;
        self.write_transaction(|tx| upsert_smart_mailbox_tx(tx, smart_mailbox, false))
    }

    fn update_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), StoreError> {
        validate_smart_mailbox(smart_mailbox)?;
        self.write_transaction(|tx| upsert_smart_mailbox_tx(tx, smart_mailbox, true))
    }

    fn delete_smart_mailbox(&self, smart_mailbox_id: &SmartMailboxId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            let changed = tx
                .execute(
                    "DELETE FROM smart_mailbox WHERE id = ?1",
                    params![smart_mailbox_id.as_str()],
                )
                .map_err(sql_to_store_error)?;
            if changed == 0 {
                return Err(StoreError::NotFound(format!(
                    "smart_mailbox:{}",
                    smart_mailbox_id.as_str()
                )));
            }
            Ok(())
        })
    }

    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, StoreError> {
        self.write_transaction(|tx| {
            upsert_default_smart_mailboxes_tx(tx)?;
            Ok(())
        })?;
        self.list_smart_mailboxes()
    }

    fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let smart_mailbox = query_smart_mailbox(&connection, smart_mailbox_id)?.ok_or_else(|| {
            StoreError::NotFound(format!("smart_mailbox:{}", smart_mailbox_id.as_str()))
        })?;
        query_messages_by_rule(&connection, &smart_mailbox.rule)
    }

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
                 JOIN account_config a
                   ON a.account_id = m.account_id
                 WHERE m.account_id = ?1 AND m.id = ?2",
            )
            .map_err(sql_to_store_error)?;

        let summary = statement
            .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
                row_to_message_summary(&connection, row)
            })
            .optional()
            .map_err(sql_to_store_error)?;

        let Some(summary) = summary else {
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

        Ok(Some(MessageDetail {
            summary,
            body_html: body.as_ref().and_then(|row| row.0.clone()),
            body_text: body.as_ref().and_then(|row| row.1.clone()),
            raw_message: body.and_then(|row| row.2),
        }))
    }

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

    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        let connection = self.read_connection()?;
        fetch_mailbox_ids(&connection, account_id, message_id)
    }

    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        let staged_bodies = batch
            .messages
            .iter()
            .map(|message| {
                let raw_mime = message
                    .raw_mime
                    .clone()
                    .or_else(|| synthesize_raw_mime(message));
                raw_mime
                    .as_deref()
                    .map(|raw_mime| self.store_raw_message(account_id, raw_mime))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.write_transaction(|tx| {
            let mut events = Vec::new();
            let mut affected_mailboxes = BTreeSet::new();
            let mut affected_threads = BTreeSet::new();
            let mut affected_conversations = BTreeSet::new();

            for mailbox_id in &batch.deleted_mailbox_ids {
                tx.execute(
                    "DELETE FROM mailbox WHERE account_id = ?1 AND id = ?2",
                    params![account_id.as_str(), mailbox_id.as_str()],
                )
                .map_err(sql_to_store_error)?;
            }

            for message_id in &batch.deleted_message_ids {
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
                    affected_mailboxes.insert(mailbox_id);
                }
                if let Some(thread_id) = thread_id {
                    affected_threads.insert(thread_id);
                }
                if let Some(conversation_id) = conversation_id {
                    affected_conversations.insert(conversation_id);
                }
            }

            for mailbox in &batch.mailboxes {
                tx.execute(
                    "INSERT INTO mailbox (account_id, id, name, role, unread_emails, total_emails)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(account_id, id) DO UPDATE SET
                        name = excluded.name,
                        role = excluded.role,
                        unread_emails = excluded.unread_emails,
                        total_emails = excluded.total_emails",
                    params![
                        account_id.as_str(),
                        mailbox.id.as_str(),
                        mailbox.name,
                        mailbox.role,
                        mailbox.unread_emails,
                        mailbox.total_emails
                    ],
                )
                .map_err(sql_to_store_error)?;
                events.push(insert_event_tx(
                    tx,
                    account_id,
                    "mailbox.updated",
                    Some(&mailbox.id),
                    None,
                    json!({ "mailboxId": mailbox.id.as_str() }),
                )?);
            }

            for (message, raw_ref) in batch.messages.iter().zip(staged_bodies.iter()) {
                let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, &message.id)?;
                let previous_keywords = fetch_keywords_tx(tx, account_id, &message.id)?;
                let previous_conversation_id = tx
                    .query_row(
                        "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
                        params![account_id.as_str(), message.id.as_str()],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(sql_to_store_error)?
                    .flatten()
                    .map(ConversationId);
                let existed = tx
                    .query_row(
                        "SELECT 1 FROM message WHERE account_id = ?1 AND id = ?2",
                        params![account_id.as_str(), message.id.as_str()],
                        |_row| Ok(()),
                    )
                    .optional()
                    .map_err(sql_to_store_error)?
                    .is_some();
                let conversation_id = assign_conversation_id_tx(tx, account_id, message)?;

                tx.execute(
                    "INSERT INTO message (
                        account_id, id, thread_id, conversation_id, remote_blob_id, subject,
                        normalized_subject, from_name, from_email, preview, received_at,
                        has_attachment, size, is_read, is_flagged, rfc_message_id, in_reply_to,
                        references_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                     ON CONFLICT(account_id, id) DO UPDATE SET
                        thread_id = excluded.thread_id,
                        conversation_id = excluded.conversation_id,
                        remote_blob_id = excluded.remote_blob_id,
                        subject = excluded.subject,
                        normalized_subject = excluded.normalized_subject,
                        from_name = excluded.from_name,
                        from_email = excluded.from_email,
                        preview = excluded.preview,
                        received_at = excluded.received_at,
                        has_attachment = excluded.has_attachment,
                        size = excluded.size,
                        is_read = excluded.is_read,
                        is_flagged = excluded.is_flagged,
                        rfc_message_id = excluded.rfc_message_id,
                        in_reply_to = excluded.in_reply_to,
                        references_json = excluded.references_json",
                    params![
                        account_id.as_str(),
                        message.id.as_str(),
                        message.source_thread_id.as_str(),
                        conversation_id.as_str(),
                        message.remote_blob_id.as_ref().map(|blob_id| blob_id.as_str()),
                        message.subject,
                        normalized_subject(message.subject.as_deref()),
                        message.from_name,
                        message.from_email,
                        message.preview,
                        message.received_at,
                        bool_to_i64(message.has_attachment),
                        message.size,
                        bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$seen")),
                        bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$flagged")),
                        message.rfc_message_id,
                        message.in_reply_to,
                        serde_json::to_string(&message.references).map_err(json_to_store_error)?
                    ],
                )
                .map_err(sql_to_store_error)?;

                tx.execute(
                    "DELETE FROM conversation_message WHERE account_id = ?1 AND message_id = ?2",
                    params![account_id.as_str(), message.id.as_str()],
                )
                .map_err(sql_to_store_error)?;
                tx.execute(
                    "INSERT INTO conversation_message (conversation_id, account_id, message_id)
                     VALUES (?1, ?2, ?3)",
                    params![conversation_id.as_str(), account_id.as_str(), message.id.as_str()],
                )
                .map_err(sql_to_store_error)?;

                tx.execute(
                    "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
                    params![account_id.as_str(), message.id.as_str()],
                )
                .map_err(sql_to_store_error)?;
                for mailbox_id in &message.mailbox_ids {
                    tx.execute(
                        "INSERT INTO message_mailbox (account_id, message_id, mailbox_id)
                         VALUES (?1, ?2, ?3)",
                        params![account_id.as_str(), message.id.as_str(), mailbox_id.as_str()],
                    )
                    .map_err(sql_to_store_error)?;
                }

                tx.execute(
                    "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
                    params![account_id.as_str(), message.id.as_str()],
                )
                .map_err(sql_to_store_error)?;
                for keyword in &message.keywords {
                    tx.execute(
                        "INSERT INTO message_keyword (account_id, message_id, keyword)
                         VALUES (?1, ?2, ?3)",
                        params![account_id.as_str(), message.id.as_str(), keyword],
                    )
                    .map_err(sql_to_store_error)?;
                }

                if message.body_html.is_some() || message.body_text.is_some() || raw_ref.is_some() {
                    upsert_body_tx(
                        tx,
                        account_id,
                        &message.id,
                        message.body_html.as_deref(),
                        message.body_text.as_deref(),
                        raw_ref.as_ref(),
                    )?;
                }

                affected_threads.insert(message.source_thread_id.clone());
                affected_conversations.insert(conversation_id.clone());
                if let Some(previous_conversation_id) = previous_conversation_id {
                    affected_conversations.insert(previous_conversation_id);
                }
                for mailbox_id in previous_mailboxes.iter().chain(message.mailbox_ids.iter()) {
                    affected_mailboxes.insert(mailbox_id.clone());
                }

                events.push(insert_event_tx(
                    tx,
                    account_id,
                    "message.updated",
                    message.mailbox_ids.first(),
                    Some(&message.id),
                    json!({
                        "messageId": message.id.as_str(),
                        "sourceThreadId": message.source_thread_id.as_str(),
                        "conversationId": conversation_id.as_str(),
                        "created": !existed
                    }),
                )?);

                if !existed || previous_keywords != message.keywords {
                    events.push(insert_event_tx(
                        tx,
                        account_id,
                        "message.keywords_changed",
                        message.mailbox_ids.first(),
                        Some(&message.id),
                        json!({
                            "messageId": message.id.as_str(),
                            "keywords": message.keywords,
                        }),
                    )?);
                }

                let current_mailboxes: BTreeSet<_> = message.mailbox_ids.iter().cloned().collect();
                let previous_mailboxes_set: BTreeSet<_> = previous_mailboxes.iter().cloned().collect();
                if !existed || current_mailboxes != previous_mailboxes_set {
                    events.push(insert_event_tx(
                        tx,
                        account_id,
                        "message.mailboxes_changed",
                        message.mailbox_ids.first(),
                        Some(&message.id),
                        json!({
                            "messageId": message.id.as_str(),
                            "mailboxIds": message.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
                        }),
                    )?);
                }

                for mailbox_id in current_mailboxes.difference(&previous_mailboxes_set) {
                    events.push(insert_event_tx(
                        tx,
                        account_id,
                        "message.arrived",
                        Some(mailbox_id),
                        Some(&message.id),
                        json!({
                            "messageId": message.id.as_str(),
                            "mailboxId": mailbox_id.as_str(),
                        }),
                    )?);
                }
            }

            for thread_id in affected_threads {
                refresh_thread_projection_tx(tx, account_id, &thread_id)?;
            }
            for conversation_id in affected_conversations {
                refresh_conversation_projection_tx(tx, &conversation_id)?;
            }
            for mailbox_id in affected_mailboxes {
                refresh_mailbox_counters_tx(tx, account_id, &mailbox_id)?;
            }
            for cursor in &batch.cursors {
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
            }

            Ok(events)
        })
    }

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
            upsert_body_tx(
                tx,
                account_id,
                message_id,
                body.body_html.as_deref(),
                body.body_text.as_deref(),
                raw_ref.as_ref(),
            )?;
            let event = insert_event_tx(
                tx,
                account_id,
                "message.body_cached",
                None,
                Some(message_id),
                json!({ "messageId": message_id.as_str() }),
            )?;
            let detail = query_message_detail_tx(tx, account_id, message_id)?
                .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
            Ok(CommandResult {
                detail: Some(detail),
                events: vec![event],
            })
        })
    }

    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| {
            let existing_keywords = fetch_keywords_tx(tx, account_id, message_id)?;
            let mut keywords: BTreeSet<_> = existing_keywords.into_iter().collect();
            for keyword in &command.add {
                keywords.insert(keyword.clone());
            }
            for keyword in &command.remove {
                keywords.remove(keyword);
            }
            tx.execute(
                "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            for keyword in &keywords {
                tx.execute(
                    "INSERT INTO message_keyword (account_id, message_id, keyword) VALUES (?1, ?2, ?3)",
                    params![account_id.as_str(), message_id.as_str(), keyword],
                )
                .map_err(sql_to_store_error)?;
            }
            tx.execute(
                "UPDATE message
                 SET is_read = ?3, is_flagged = ?4
                 WHERE account_id = ?1 AND id = ?2",
                params![
                    account_id.as_str(),
                    message_id.as_str(),
                    bool_to_i64(keywords.contains("$seen")),
                    bool_to_i64(keywords.contains("$flagged"))
                ],
            )
            .map_err(sql_to_store_error)?;

            let mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
            for mailbox_id in &mailboxes {
                refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
            }
            let event = insert_event_tx(
                tx,
                account_id,
                "message.keywords_changed",
                mailboxes.first(),
                Some(message_id),
                json!({ "messageId": message_id.as_str(), "keywords": keywords.iter().cloned().collect::<Vec<_>>() }),
            )?;
            let detail = query_message_detail_tx(tx, account_id, message_id)?
                .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
            Ok(CommandResult {
                detail: Some(detail),
                events: vec![event],
            })
        })
    }

    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| {
            let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
            tx.execute(
                "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            for mailbox_id in &command.mailbox_ids {
                tx.execute(
                    "INSERT INTO message_mailbox (account_id, message_id, mailbox_id) VALUES (?1, ?2, ?3)",
                    params![account_id.as_str(), message_id.as_str(), mailbox_id.as_str()],
                )
                .map_err(sql_to_store_error)?;
            }

            let previous_set: BTreeSet<_> = previous_mailboxes.iter().cloned().collect();
            let current_set: BTreeSet<_> = command.mailbox_ids.iter().cloned().collect();

            for mailbox_id in previous_set.union(&current_set) {
                refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
            }

            let mut events = Vec::new();
            events.push(insert_event_tx(
                tx,
                account_id,
                "message.mailboxes_changed",
                command.mailbox_ids.first(),
                Some(message_id),
                json!({
                    "messageId": message_id.as_str(),
                    "mailboxIds": command.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
                }),
            )?);
            for mailbox_id in current_set.difference(&previous_set) {
                events.push(insert_event_tx(
                    tx,
                    account_id,
                    "message.arrived",
                    Some(mailbox_id),
                    Some(message_id),
                    json!({ "messageId": message_id.as_str(), "mailboxId": mailbox_id.as_str() }),
                )?);
            }

            let detail = query_message_detail_tx(tx, account_id, message_id)?
                .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
            Ok(CommandResult {
                detail: Some(detail),
                events,
            })
        })
    }

    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| {
            let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
            let thread_id = tx
                .query_row(
                    "SELECT thread_id FROM message WHERE account_id = ?1 AND id = ?2",
                    params![account_id.as_str(), message_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(sql_to_store_error)?
                .map(ThreadId)
                .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
            delete_message_tx(tx, account_id, message_id)?;
            refresh_thread_projection_tx(tx, account_id, &thread_id)?;
            for mailbox_id in &previous_mailboxes {
                refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
            }
            let event = insert_event_tx(
                tx,
                account_id,
                "message.updated",
                previous_mailboxes.first(),
                Some(message_id),
                json!({ "messageId": message_id.as_str(), "deleted": true }),
            )?;
            Ok(CommandResult {
                detail: None,
                events: vec![event],
            })
        })
    }

    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
        let connection = self.read_connection()?;
        let mut sql = "SELECT seq, account_id, topic, occurred_at, mailbox_id, message_id, payload
             FROM event_log
             WHERE 1 = 1"
            .to_string();
        let mut bindings: Vec<String> = Vec::new();

        if let Some(account_id) = &filter.account_id {
            sql.push_str(" AND account_id = ?");
            sql.push_str(&(bindings.len() + 1).to_string());
            bindings.push(account_id.to_string());
        }

        if let Some(after_seq) = filter.after_seq {
            sql.push_str(" AND seq > ?");
            sql.push_str(&(bindings.len() + 1).to_string());
            bindings.push(after_seq.to_string());
        }
        if let Some(topic) = &filter.topic {
            sql.push_str(" AND topic = ?");
            sql.push_str(&(bindings.len() + 1).to_string());
            bindings.push(topic.clone());
        }
        if let Some(mailbox_id) = &filter.mailbox_id {
            sql.push_str(" AND mailbox_id = ?");
            sql.push_str(&(bindings.len() + 1).to_string());
            bindings.push(mailbox_id.to_string());
        }
        sql.push_str(" ORDER BY seq ASC");

        let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
        let params_ref = rusqlite::params_from_iter(bindings.iter());
        let rows = statement
            .query_map(params_ref, row_to_event)
            .map_err(sql_to_store_error)?;
        let events = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(events)
    }

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

fn init_schema(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS app_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                settings_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS account_config (
                account_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                driver TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                transport_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS smart_mailbox (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL,
                default_key TEXT,
                parent_id TEXT,
                rule_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS smart_mailbox_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                initialized_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mailbox (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                role TEXT,
                unread_emails INTEGER NOT NULL DEFAULT 0,
                total_emails INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS message (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                conversation_id TEXT,
                remote_blob_id TEXT,
                subject TEXT,
                normalized_subject TEXT,
                from_name TEXT,
                from_email TEXT,
                preview TEXT,
                received_at TEXT NOT NULL,
                has_attachment INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                is_read INTEGER NOT NULL DEFAULT 1,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                rfc_message_id TEXT,
                in_reply_to TEXT,
                references_json TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS conversation (
                id TEXT PRIMARY KEY,
                subject TEXT,
                normalized_subject TEXT,
                latest_received_at TEXT NOT NULL,
                latest_source_id TEXT NOT NULL,
                latest_message_id TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                unread_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS conversation_message (
                conversation_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                PRIMARY KEY (conversation_id, account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS message_mailbox (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS message_keyword (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, keyword)
            );

            CREATE TABLE IF NOT EXISTS message_body (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                body_html TEXT,
                body_text TEXT,
                raw_path TEXT,
                raw_sha256 TEXT,
                raw_size INTEGER,
                raw_mime_type TEXT,
                fetched_at TEXT,
                PRIMARY KEY (account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS thread_view (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                email_ids TEXT NOT NULL,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS sync_cursor (
                account_id TEXT NOT NULL,
                object_type TEXT NOT NULL,
                state TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, object_type)
            );

            CREATE TABLE IF NOT EXISTS event_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                mailbox_id TEXT,
                message_id TEXT,
                payload TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_message_thread
                ON message (account_id, thread_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_conversation
                ON message (conversation_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_rfc_message_id
                ON message (rfc_message_id);
            CREATE INDEX IF NOT EXISTS idx_message_mailbox
                ON message_mailbox (account_id, mailbox_id);
            CREATE INDEX IF NOT EXISTS idx_message_keyword
                ON message_keyword (account_id, keyword);
            CREATE INDEX IF NOT EXISTS idx_event_log_lookup
                ON event_log (account_id, topic, mailbox_id, seq);
            CREATE INDEX IF NOT EXISTS idx_conversation_message_lookup
                ON conversation_message (account_id, message_id);
            CREATE INDEX IF NOT EXISTS idx_smart_mailbox_position
                ON smart_mailbox (position, name);
            ",
        )
        .map_err(sql_to_store_error)?;
    ensure_projection_columns(connection)?;
    Ok(())
}

fn row_to_account_settings(row: &rusqlite::Row<'_>) -> Result<AccountSettings, rusqlite::Error> {
    let transport_json: String = row.get(4)?;
    let transport = serde_json::from_str::<AccountTransportSettings>(&transport_json)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    Ok(AccountSettings {
        id: AccountId(row.get(0)?),
        name: row.get(1)?,
        driver: parse_account_driver(&row.get::<_, String>(2)?)?,
        enabled: row.get::<_, i64>(3)? != 0,
        transport,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn ensure_projection_columns(connection: &Connection) -> Result<(), StoreError> {
    ensure_column(
        connection,
        "message",
        "conversation_id",
        "ALTER TABLE message ADD COLUMN conversation_id TEXT",
    )?;
    ensure_column(
        connection,
        "message",
        "normalized_subject",
        "ALTER TABLE message ADD COLUMN normalized_subject TEXT",
    )?;
    ensure_column(
        connection,
        "message",
        "rfc_message_id",
        "ALTER TABLE message ADD COLUMN rfc_message_id TEXT",
    )?;
    ensure_column(
        connection,
        "message",
        "in_reply_to",
        "ALTER TABLE message ADD COLUMN in_reply_to TEXT",
    )?;
    ensure_column(
        connection,
        "message",
        "references_json",
        "ALTER TABLE message ADD COLUMN references_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    Ok(())
}

fn seed_default_smart_mailboxes(connection: &Connection) -> Result<(), StoreError> {
    let initialized = connection
        .query_row(
            "SELECT initialized_at FROM smart_mailbox_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .is_some();
    if initialized {
        return Ok(());
    }

    let tx = connection.unchecked_transaction().map_err(sql_to_store_error)?;
    upsert_default_smart_mailboxes_tx(&tx)?;
    tx.execute(
        "INSERT INTO smart_mailbox_state (singleton, initialized_at) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET initialized_at = initialized_at",
        params![now_iso8601()?],
    )
    .map_err(sql_to_store_error)?;
    tx.commit().map_err(sql_to_store_error)?;
    Ok(())
}

fn default_smart_mailboxes() -> Vec<SmartMailbox> {
    let timestamp = "1970-01-01T00:00:00Z".to_string();
    vec![
        default_smart_mailbox("default-inbox", "Inbox", 0, "inbox", Some("inbox")),
        default_smart_mailbox("default-archive", "Archive", 1, "archive", Some("archive")),
        default_smart_mailbox("default-drafts", "Drafts", 2, "drafts", Some("drafts")),
        default_smart_mailbox("default-sent", "Sent", 3, "sent", Some("sent")),
        default_smart_mailbox("default-junk", "Junk", 4, "junk", Some("junk")),
        default_smart_mailbox("default-trash", "Trash", 5, "trash", Some("trash")),
        SmartMailbox {
            id: SmartMailboxId::from("default-all-mail"),
            name: "All Mail".to_string(),
            position: 6,
            kind: SmartMailboxKind::Default,
            default_key: Some("all-mail".to_string()),
            parent_id: None,
            rule: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: Vec::new(),
                },
            },
            created_at: timestamp.clone(),
            updated_at: timestamp,
        },
    ]
}

fn default_smart_mailbox(
    id: &str,
    name: &str,
    position: i64,
    default_key: &str,
    role: Option<&str>,
) -> SmartMailbox {
    let timestamp = "1970-01-01T00:00:00Z".to_string();
    let nodes = role
        .map(|value| {
            vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxRole,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            })]
        })
        .unwrap_or_default();
    SmartMailbox {
        id: SmartMailboxId::from(id),
        name: name.to_string(),
        position,
        kind: SmartMailboxKind::Default,
        default_key: Some(default_key.to_string()),
        parent_id: None,
        rule: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes,
            },
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    }
}

fn upsert_default_smart_mailboxes_tx(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let timestamp = now_iso8601()?;
    for mailbox in default_smart_mailboxes() {
        tx.execute(
            "INSERT INTO smart_mailbox (
                id, name, position, kind, default_key, parent_id, rule_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                position = excluded.position,
                kind = excluded.kind,
                default_key = excluded.default_key,
                parent_id = excluded.parent_id,
                rule_json = excluded.rule_json,
                updated_at = excluded.updated_at",
            params![
                mailbox.id.as_str(),
                mailbox.name,
                mailbox.position,
                smart_mailbox_kind_to_str(&mailbox.kind),
                mailbox.default_key,
                mailbox.parent_id.as_ref().map(SmartMailboxId::as_str),
                serde_json::to_string(&mailbox.rule).map_err(json_to_store_error)?,
                mailbox.created_at,
                timestamp,
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

fn smart_mailbox_kind_to_str(kind: &SmartMailboxKind) -> &'static str {
    match kind {
        SmartMailboxKind::Default => "default",
        SmartMailboxKind::User => "user",
    }
}

fn parse_smart_mailbox_kind(value: &str) -> Result<SmartMailboxKind, rusqlite::Error> {
    match value {
        "default" => Ok(SmartMailboxKind::Default),
        "user" => Ok(SmartMailboxKind::User),
        _ => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
            StoreError::Failure(format!("unsupported smart mailbox kind: {value}")),
        ))),
    }
}

fn load_smart_mailboxes(connection: &Connection) -> Result<Vec<SmartMailbox>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, name, position, kind, default_key, parent_id, rule_json, created_at, updated_at
             FROM smart_mailbox
             ORDER BY position ASC, name ASC, id ASC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map([], row_to_smart_mailbox)
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql_to_store_error)
}

fn row_to_smart_mailbox(row: &rusqlite::Row<'_>) -> Result<SmartMailbox, rusqlite::Error> {
    let rule_json: String = row.get(6)?;
    let rule = serde_json::from_str::<SmartMailboxRule>(&rule_json)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    Ok(SmartMailbox {
        id: SmartMailboxId(row.get(0)?),
        name: row.get(1)?,
        position: row.get(2)?,
        kind: parse_smart_mailbox_kind(&row.get::<_, String>(3)?)?,
        default_key: row.get(4)?,
        parent_id: row.get::<_, Option<String>>(5)?.map(SmartMailboxId),
        rule,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn query_smart_mailbox(
    connection: &Connection,
    smart_mailbox_id: &SmartMailboxId,
) -> Result<Option<SmartMailbox>, StoreError> {
    connection
        .query_row(
            "SELECT id, name, position, kind, default_key, parent_id, rule_json, created_at, updated_at
             FROM smart_mailbox
             WHERE id = ?1",
            params![smart_mailbox_id.as_str()],
            row_to_smart_mailbox,
        )
        .optional()
        .map_err(sql_to_store_error)
}

fn validate_smart_mailbox(smart_mailbox: &SmartMailbox) -> Result<(), StoreError> {
    if smart_mailbox.id.as_str().trim().is_empty() {
        return Err(StoreError::Failure("smart mailbox id is required".to_string()));
    }
    if smart_mailbox.name.trim().is_empty() {
        return Err(StoreError::Failure("smart mailbox name is required".to_string()));
    }
    if matches!(smart_mailbox.kind, SmartMailboxKind::Default) && smart_mailbox.default_key.is_none()
    {
        return Err(StoreError::Failure(
            "default smart mailboxes must include default_key".to_string(),
        ));
    }
    validate_smart_mailbox_group(&smart_mailbox.rule.root)
}

fn validate_smart_mailbox_group(group: &SmartMailboxGroup) -> Result<(), StoreError> {
    for node in &group.nodes {
        match node {
            SmartMailboxRuleNode::Group(child) => validate_smart_mailbox_group(child)?,
            SmartMailboxRuleNode::Condition(condition) => validate_smart_mailbox_condition(condition)?,
        }
    }
    Ok(())
}

fn validate_smart_mailbox_condition(condition: &SmartMailboxCondition) -> Result<(), StoreError> {
    use SmartMailboxField as Field;
    use SmartMailboxOperator as Op;
    match condition.field {
        Field::SourceId | Field::SourceName | Field::MailboxId | Field::MailboxRole | Field::Keyword => {
            match condition.operator {
                Op::Equals => {
                    expect_string_value(&condition.value)?;
                }
                Op::In => {
                    expect_strings_value(&condition.value)?;
                }
                _ => return Err(StoreError::Failure(format!(
                    "unsupported operator {:?} for field {:?}",
                    condition.operator, condition.field
                ))),
            }
        }
        Field::IsRead | Field::IsFlagged | Field::HasAttachment => match condition.operator {
            Op::Equals => {
                expect_bool_value(&condition.value)?;
            }
            _ => {
                return Err(StoreError::Failure(format!(
                    "unsupported operator {:?} for field {:?}",
                    condition.operator, condition.field
                )))
            }
        },
        Field::FromName | Field::FromEmail | Field::Subject | Field::Preview => match condition.operator {
            Op::Equals | Op::Contains => {
                expect_string_value(&condition.value)?;
            }
            Op::In => {
                expect_strings_value(&condition.value)?;
            }
            _ => {
                return Err(StoreError::Failure(format!(
                    "unsupported operator {:?} for field {:?}",
                    condition.operator, condition.field
                )))
            }
        },
        Field::ReceivedAt => match condition.operator {
            Op::Before | Op::After | Op::OnOrBefore | Op::OnOrAfter => {
                expect_string_value(&condition.value)?;
            }
            _ => {
                return Err(StoreError::Failure(format!(
                    "unsupported operator {:?} for field {:?}",
                    condition.operator, condition.field
                )))
            }
        },
    }
    Ok(())
}

fn expect_string_value(value: &SmartMailboxValue) -> Result<&str, StoreError> {
    match value {
        SmartMailboxValue::String(value) => Ok(value.as_str()),
        _ => Err(StoreError::Failure("expected string smart mailbox value".to_string())),
    }
}

fn expect_strings_value(value: &SmartMailboxValue) -> Result<&[String], StoreError> {
    match value {
        SmartMailboxValue::Strings(values) => Ok(values.as_slice()),
        _ => Err(StoreError::Failure("expected string array smart mailbox value".to_string())),
    }
}

fn expect_bool_value(value: &SmartMailboxValue) -> Result<bool, StoreError> {
    match value {
        SmartMailboxValue::Bool(value) => Ok(*value),
        _ => Err(StoreError::Failure("expected boolean smart mailbox value".to_string())),
    }
}

fn upsert_smart_mailbox_tx(
    tx: &Transaction<'_>,
    smart_mailbox: &SmartMailbox,
    update_existing: bool,
) -> Result<(), StoreError> {
    let rule_json = serde_json::to_string(&smart_mailbox.rule).map_err(json_to_store_error)?;
    if update_existing {
        let changed = tx
            .execute(
                "UPDATE smart_mailbox
                 SET name = ?2,
                     position = ?3,
                     kind = ?4,
                     default_key = ?5,
                     parent_id = ?6,
                     rule_json = ?7,
                     updated_at = ?8
                 WHERE id = ?1",
                params![
                    smart_mailbox.id.as_str(),
                    smart_mailbox.name,
                    smart_mailbox.position,
                    smart_mailbox_kind_to_str(&smart_mailbox.kind),
                    smart_mailbox.default_key,
                    smart_mailbox.parent_id.as_ref().map(SmartMailboxId::as_str),
                    rule_json,
                    smart_mailbox.updated_at,
                ],
            )
            .map_err(sql_to_store_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "smart_mailbox:{}",
                smart_mailbox.id.as_str()
            )));
        }
    } else {
        tx.execute(
            "INSERT INTO smart_mailbox (
                id, name, position, kind, default_key, parent_id, rule_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                smart_mailbox.id.as_str(),
                smart_mailbox.name,
                smart_mailbox.position,
                smart_mailbox_kind_to_str(&smart_mailbox.kind),
                smart_mailbox.default_key,
                smart_mailbox.parent_id.as_ref().map(SmartMailboxId::as_str),
                rule_json,
                smart_mailbox.created_at,
                smart_mailbox.updated_at,
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

fn count_smart_mailbox_messages(
    connection: &Connection,
    rule: &SmartMailboxRule,
) -> Result<(i64, i64), StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT COUNT(*), SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END)
         FROM message m
         JOIN account_config a ON a.account_id = m.account_id
         WHERE a.enabled = 1 AND ({where_clause})"
    );
    connection
        .query_row(&sql, params_from_iter(params), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            ))
        })
        .map_err(sql_to_store_error)
}

fn query_messages_by_rule(
    connection: &Connection,
    rule: &SmartMailboxRule,
) -> Result<Vec<MessageSummary>, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                m.is_read, m.is_flagged
         FROM message m
         JOIN account_config a ON a.account_id = m.account_id
         WHERE a.enabled = 1 AND ({where_clause})
         ORDER BY m.received_at DESC"
    );
    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params_from_iter(params), |row| row_to_message_summary(connection, row))
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql_to_store_error)
}

fn compile_smart_mailbox_rule(
    rule: &SmartMailboxRule,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    compile_smart_mailbox_group(&rule.root, params)
}

fn compile_smart_mailbox_group(
    group: &SmartMailboxGroup,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    if group.nodes.is_empty() {
        return Ok(if group.negated {
            "NOT (1 = 1)".to_string()
        } else {
            "1 = 1".to_string()
        });
    }
    let joiner = match group.operator {
        SmartMailboxGroupOperator::All => " AND ",
        SmartMailboxGroupOperator::Any => " OR ",
    };
    let mut parts = Vec::with_capacity(group.nodes.len());
    for node in &group.nodes {
        let fragment = match node {
            SmartMailboxRuleNode::Group(group) => compile_smart_mailbox_group(group, params)?,
            SmartMailboxRuleNode::Condition(condition) => {
                compile_smart_mailbox_condition(condition, params)?
            }
        };
        parts.push(format!("({fragment})"));
    }
    let combined = parts.join(joiner);
    Ok(if group.negated {
        format!("NOT ({combined})")
    } else {
        combined
    })
}

fn compile_smart_mailbox_condition(
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let fragment = match condition.field {
        SmartMailboxField::SourceId => compile_simple_field("m.account_id", condition, params)?,
        SmartMailboxField::SourceName => compile_simple_field("a.name", condition, params)?,
        SmartMailboxField::FromName => compile_text_field("m.from_name", condition, params)?,
        SmartMailboxField::FromEmail => compile_text_field("m.from_email", condition, params)?,
        SmartMailboxField::Subject => compile_text_field("m.subject", condition, params)?,
        SmartMailboxField::Preview => compile_text_field("m.preview", condition, params)?,
        SmartMailboxField::ReceivedAt => compile_date_field("m.received_at", condition, params)?,
        SmartMailboxField::IsRead => compile_bool_field("m.is_read", condition)?,
        SmartMailboxField::IsFlagged => compile_bool_field("m.is_flagged", condition)?,
        SmartMailboxField::HasAttachment => compile_bool_field("m.has_attachment", condition)?,
        SmartMailboxField::MailboxId => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox mm
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND mm.mailbox_id",
            condition,
            params,
        )?,
        SmartMailboxField::Keyword => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_keyword mk
                WHERE mk.account_id = m.account_id
                  AND mk.message_id = m.id
                  AND mk.keyword",
            condition,
            params,
        )?,
        SmartMailboxField::MailboxRole => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox mm
                JOIN mailbox b
                  ON b.account_id = mm.account_id
                 AND b.id = mm.mailbox_id
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND b.role",
            condition,
            params,
        )?,
    };
    Ok(if condition.negated {
        format!("NOT ({fragment})")
    } else {
        fragment
    })
}

fn compile_simple_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(expect_string_value(&condition.value)?.to_string()));
            Ok(format!("{column} = ?"))
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            compile_in_clause(column, values, params)
        }
        _ => Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        ))),
    }
}

fn compile_text_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(expect_string_value(&condition.value)?.to_string()));
            Ok(format!("COALESCE({column}, '') = ?"))
        }
        SmartMailboxOperator::Contains => {
            params.push(SqlValue::Text(format!(
                "%{}%",
                expect_string_value(&condition.value)?.to_lowercase()
            )));
            Ok(format!("LOWER(COALESCE({column}, '')) LIKE ?"))
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            compile_in_clause(&format!("COALESCE({column}, '')"), values, params)
        }
        _ => Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        ))),
    }
}

fn compile_date_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    params.push(SqlValue::Text(expect_string_value(&condition.value)?.to_string()));
    let comparator = match condition.operator {
        SmartMailboxOperator::Before => "<",
        SmartMailboxOperator::After => ">",
        SmartMailboxOperator::OnOrBefore => "<=",
        SmartMailboxOperator::OnOrAfter => ">=",
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!("{column} {comparator} ?"))
}

fn compile_bool_field(column: &str, condition: &SmartMailboxCondition) -> Result<String, StoreError> {
    if !matches!(condition.operator, SmartMailboxOperator::Equals) {
        return Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        )));
    }
    let expected = if expect_bool_value(&condition.value)? { 1 } else { 0 };
    Ok(format!("{column} = {expected}"))
}

fn compile_exists_membership(
    prefix: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let suffix = match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(expect_string_value(&condition.value)?.to_string()));
            " = ?".to_string()
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            let placeholders = push_placeholders(values, params);
            format!(" IN ({placeholders})")
        }
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!("{prefix}{suffix}\n            )"))
}

fn compile_in_clause(
    column: &str,
    values: &[String],
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    if values.is_empty() {
        return Ok("1 = 0".to_string());
    }
    let placeholders = push_placeholders(values, params);
    Ok(format!("{column} IN ({placeholders})"))
}

fn push_placeholders(values: &[String], params: &mut Vec<SqlValue>) -> String {
    for value in values {
        params.push(SqlValue::Text(value.clone()));
    }
    vec!["?"; values.len()].join(", ")
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    sql: &str,
) -> Result<(), StoreError> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&pragma).map_err(sql_to_store_error)?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?
        .into_iter()
        .any(|existing| existing == column);
    if !exists {
        connection.execute(sql, []).map_err(sql_to_store_error)?;
    }
    Ok(())
}

fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    connection
        .pragma_update(None, "journal_mode", "wal")
        .map_err(sql_to_store_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(sql_to_store_error)?;
    Ok(())
}

fn row_to_message_summary(
    connection: &Connection,
    row: &rusqlite::Row<'_>,
) -> Result<MessageSummary, rusqlite::Error> {
    let message_id = MessageId(row.get(0)?);
    let source_id = AccountId(row.get(1)?);
    let mailbox_ids =
        fetch_mailbox_ids(connection, &source_id, &message_id).map_err(store_to_sqlite_error)?;
    let keywords =
        fetch_keywords(connection, &source_id, &message_id).map_err(store_to_sqlite_error)?;
    Ok(MessageSummary {
        id: message_id.clone(),
        source_id,
        source_name: row.get(2)?,
        source_thread_id: ThreadId(row.get(3)?),
        conversation_id: ConversationId(row.get(4)?),
        subject: row.get(5)?,
        from_name: row.get(6)?,
        from_email: row.get(7)?,
        preview: row.get(8)?,
        received_at: row.get(9)?,
        has_attachment: row.get::<_, i64>(10)? != 0,
        is_read: row.get::<_, i64>(11)? != 0,
        is_flagged: row.get::<_, i64>(12)? != 0,
        mailbox_ids,
        keywords,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> Result<DomainEvent, rusqlite::Error> {
    let payload: String = row.get(6)?;
    Ok(DomainEvent {
        seq: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        topic: row.get(2)?,
        occurred_at: row.get(3)?,
        mailbox_id: row.get::<_, Option<String>>(4)?.map(MailboxId),
        message_id: row.get::<_, Option<String>>(5)?.map(MessageId),
        payload: serde_json::from_str(&payload).unwrap_or_else(|_| json!({})),
    })
}

fn fetch_mailbox_ids(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    let mailbox_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(mailbox_ids)
}

fn fetch_mailbox_ids_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    let mailbox_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(mailbox_ids)
}

fn fetch_keywords(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT keyword
             FROM message_keyword
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get(0)
        })
        .map_err(sql_to_store_error)?;
    let keywords = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(keywords)
}

fn fetch_keywords_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT keyword
             FROM message_keyword
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get(0)
        })
        .map_err(sql_to_store_error)?;
    let keywords = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    Ok(keywords)
}

fn query_message_detail_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<MessageDetail>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN account_config a
               ON a.account_id = m.account_id
             WHERE m.account_id = ?1 AND m.id = ?2",
        )
        .map_err(sql_to_store_error)?;

    let detail = statement
        .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
            let summary = MessageSummary {
                id: MessageId(row.get(0)?),
                source_id: AccountId(row.get(1)?),
                source_name: row.get(2)?,
                source_thread_id: ThreadId(row.get(3)?),
                conversation_id: ConversationId(row.get(4)?),
                subject: row.get(5)?,
                from_name: row.get(6)?,
                from_email: row.get(7)?,
                preview: row.get(8)?,
                received_at: row.get(9)?,
                has_attachment: row.get::<_, i64>(10)? != 0,
                is_read: row.get::<_, i64>(11)? != 0,
                is_flagged: row.get::<_, i64>(12)? != 0,
                mailbox_ids: fetch_mailbox_ids_tx(tx, account_id, message_id)
                    .map_err(store_to_sqlite_error)?,
                keywords: fetch_keywords_tx(tx, account_id, message_id)
                    .map_err(store_to_sqlite_error)?,
            };
            Ok(summary)
        })
        .optional()
        .map_err(sql_to_store_error)?;

    let Some(summary) = detail else {
        return Ok(None);
    };

    let body = tx
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

    Ok(Some(MessageDetail {
        summary,
        body_html: body.as_ref().and_then(|tuple| tuple.0.clone()),
        body_text: body.as_ref().and_then(|tuple| tuple.1.clone()),
        raw_message: body.and_then(|tuple| tuple.2),
    }))
}

fn assign_conversation_id_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &mail_domain::MessageRecord,
) -> Result<ConversationId, StoreError> {
    let mut matches = HashSet::new();
    let mut header_ids = Vec::new();
    if let Some(message_id) = normalize_header_token(message.rfc_message_id.as_deref()) {
        header_ids.push(message_id);
    }
    if let Some(in_reply_to) = normalize_header_token(message.in_reply_to.as_deref()) {
        header_ids.push(in_reply_to);
    }
    for reference in &message.references {
        if let Some(reference) = normalize_header_token(Some(reference.as_str())) {
            header_ids.push(reference);
        }
    }
    for header_id in header_ids {
        let mut statement = tx
            .prepare(
                "SELECT DISTINCT conversation_id
                 FROM message
                 WHERE rfc_message_id = ?1 AND conversation_id IS NOT NULL",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![header_id], |row| row.get::<_, String>(0))
            .map_err(sql_to_store_error)?;
        for conversation_id in rows {
            matches.insert(conversation_id.map_err(sql_to_store_error)?);
        }
    }

    if matches.is_empty() {
        let by_thread = tx
            .query_row(
                "SELECT conversation_id
                 FROM message
                 WHERE account_id = ?1 AND thread_id = ?2 AND conversation_id IS NOT NULL
                 ORDER BY received_at DESC
                 LIMIT 1",
                params![account_id.as_str(), message.source_thread_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        if let Some(conversation_id) = by_thread {
            matches.insert(conversation_id);
        }
    }

    if matches.is_empty() {
        if let Some(normalized_subject_value) = normalized_subject(message.subject.as_deref()) {
            let by_subject = tx
                .query_row(
                    "SELECT conversation_id
                     FROM message
                     WHERE account_id = ?1
                       AND normalized_subject = ?2
                       AND conversation_id IS NOT NULL
                     ORDER BY received_at DESC
                     LIMIT 1",
                    params![account_id.as_str(), normalized_subject_value],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(sql_to_store_error)?;
            if let Some(conversation_id) = by_subject {
                matches.insert(conversation_id);
            }
        }
    }

    let mut matches = matches.into_iter().collect::<Vec<_>>();
    matches.sort();
    let target = matches
        .first()
        .map(|conversation_id| ConversationId::from(conversation_id.as_str()))
        .unwrap_or_else(|| generate_conversation_id(account_id, message));
    if matches.len() > 1 {
        merge_conversations_tx(tx, &target, &matches[1..])?;
    }
    Ok(target)
}

fn merge_conversations_tx(
    tx: &Transaction<'_>,
    target: &ConversationId,
    other_ids: &[String],
) -> Result<(), StoreError> {
    for other_id in other_ids {
        tx.execute(
            "UPDATE message
             SET conversation_id = ?1
             WHERE conversation_id = ?2",
            params![target.as_str(), other_id],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "UPDATE OR REPLACE conversation_message
             SET conversation_id = ?1
             WHERE conversation_id = ?2",
            params![target.as_str(), other_id],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "DELETE FROM conversation WHERE id = ?1",
            params![other_id],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

fn refresh_conversation_projection_tx(
    tx: &Transaction<'_>,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT m.account_id, m.id, m.subject, m.normalized_subject, m.received_at, m.is_read
             FROM conversation_message cm
             JOIN message m
               ON m.account_id = cm.account_id
              AND m.id = cm.message_id
             WHERE cm.conversation_id = ?1
             ORDER BY m.received_at DESC, m.id DESC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![conversation_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;

    if rows.is_empty() {
        tx.execute(
            "DELETE FROM conversation WHERE id = ?1",
            params![conversation_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        return Ok(());
    }

    let latest = &rows[0];
    let subject = latest
        .2
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.2.clone()));
    let normalized_subject_value = latest
        .3
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.3.clone()));
    let unread_count = rows.iter().filter(|row| row.5 == 0).count() as i64;
    tx.execute(
        "INSERT INTO conversation (
            id, subject, normalized_subject, latest_received_at, latest_source_id,
            latest_message_id, message_count, unread_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject,
            normalized_subject = excluded.normalized_subject,
            latest_received_at = excluded.latest_received_at,
            latest_source_id = excluded.latest_source_id,
            latest_message_id = excluded.latest_message_id,
            message_count = excluded.message_count,
            unread_count = excluded.unread_count",
        params![
            conversation_id.as_str(),
            subject,
            normalized_subject_value,
            &latest.4,
            &latest.0,
            &latest.1,
            rows.len() as i64,
            unread_count,
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn cleanup_orphan_conversations_tx(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM conversation
         WHERE id NOT IN (SELECT DISTINCT conversation_id FROM conversation_message)",
        [],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn refresh_thread_projection_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT id
             FROM message
             WHERE account_id = ?1 AND thread_id = ?2
             ORDER BY received_at ASC",
        )
        .map_err(sql_to_store_error)?;
    let email_ids = statement
        .query_map(params![account_id.as_str(), thread_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    if email_ids.is_empty() {
        tx.execute(
            "DELETE FROM thread_view WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), thread_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
    } else {
        tx.execute(
            "INSERT INTO thread_view (account_id, id, email_ids)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, id) DO UPDATE SET email_ids = excluded.email_ids",
            params![
                account_id.as_str(),
                thread_id.as_str(),
                serde_json::to_string(&email_ids)
                    .map_err(|err| StoreError::Failure(err.to_string()))?
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

fn refresh_mailbox_counters_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
) -> Result<(), StoreError> {
    let (total, unread) = tx
        .query_row(
            "SELECT COUNT(*), SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END)
             FROM message_mailbox mm
             JOIN message m
               ON m.account_id = mm.account_id
              AND m.id = mm.message_id
             WHERE mm.account_id = ?1 AND mm.mailbox_id = ?2",
            params![account_id.as_str(), mailbox_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )
        .map_err(sql_to_store_error)?;
    tx.execute(
        "UPDATE mailbox
         SET total_emails = ?3,
             unread_emails = ?4
         WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), mailbox_id.as_str(), total, unread],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn upsert_body_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body_html: Option<&str>,
    body_text: Option<&str>,
    raw_ref: Option<&RawMessageRef>,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO message_body (
            account_id, message_id, body_html, body_text, raw_path, raw_sha256, raw_size, raw_mime_type, fetched_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            body_html = excluded.body_html,
            body_text = excluded.body_text,
            raw_path = excluded.raw_path,
            raw_sha256 = excluded.raw_sha256,
            raw_size = excluded.raw_size,
            raw_mime_type = excluded.raw_mime_type,
            fetched_at = excluded.fetched_at",
        params![
            account_id.as_str(),
            message_id.as_str(),
            body_html,
            body_text,
            raw_ref.map(|raw| raw.path.as_str()),
            raw_ref.map(|raw| raw.sha256.as_str()),
            raw_ref.map(|raw| raw.size),
            raw_ref.map(|raw| raw.mime_type.as_str()),
            raw_ref.map(|raw| raw.fetched_at.as_str()),
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn delete_message_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
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

fn insert_event_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    topic: &str,
    mailbox_id: Option<&MailboxId>,
    message_id: Option<&MessageId>,
    payload: Value,
) -> Result<DomainEvent, StoreError> {
    let occurred_at = now_iso8601()?;
    tx.execute(
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
        occurred_at,
        mailbox_id: mailbox_id.cloned(),
        message_id: message_id.cloned(),
        payload,
    })
}

fn split_group_concat_ids(value: Option<String>) -> Vec<AccountId> {
    split_group_concat_strings(value)
        .into_iter()
        .map(AccountId)
        .collect()
}

fn split_group_concat_strings(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalize_header_token(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalized_subject(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let mut normalized = value.trim();
        loop {
            let lower = normalized.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("re:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            if let Some(rest) = lower.strip_prefix("fwd:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            break;
        }
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_ascii_lowercase())
        }
    })
}

fn generate_conversation_id(
    account_id: &AccountId,
    message: &mail_domain::MessageRecord,
) -> ConversationId {
    let mut hasher = Sha256::new();
    hasher.update(account_id.as_str().as_bytes());
    hasher.update(message.id.as_str().as_bytes());
    if let Some(rfc_message_id) = &message.rfc_message_id {
        hasher.update(rfc_message_id.as_bytes());
    }
    if let Some(subject) = &message.subject {
        hasher.update(subject.as_bytes());
    }
    ConversationId(format!("conv-{}", hex_encode(hasher.finalize())))
}

fn synthesize_raw_mime(message: &mail_domain::MessageRecord) -> Option<String> {
    if message.body_html.is_none() && message.body_text.is_none() {
        return None;
    }
    let subject = message.subject.as_deref().unwrap_or("(no subject)");
    let from = match (&message.from_name, &message.from_email) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (None, Some(email)) => email.clone(),
        _ => "unknown@example.invalid".to_string(),
    };
    let text = message
        .body_text
        .as_deref()
        .unwrap_or(message.preview.as_deref().unwrap_or(""));
    Some(format!(
        "From: {from}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n"
    ))
}

fn parse_sync_object(value: &str) -> Result<SyncObject, rusqlite::Error> {
    match value {
        "mailbox" => Ok(SyncObject::Mailbox),
        "message" => Ok(SyncObject::Message),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(StoreError::Failure(format!("unknown sync object {other}"))),
        )),
    }
}

fn parse_account_driver(value: &str) -> Result<AccountDriver, rusqlite::Error> {
    match value {
        "jmap" => Ok(AccountDriver::Jmap),
        "mock" => Ok(AccountDriver::Mock),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(StoreError::Failure(format!(
                "unknown account driver {other}"
            ))),
        )),
    }
}

fn now_iso8601() -> Result<String, StoreError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| StoreError::Failure(err.to_string()))
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn sql_to_store_error(err: rusqlite::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

fn store_to_sqlite_error(err: StoreError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}

fn io_to_store_error(err: std::io::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

fn json_to_store_error(err: impl std::error::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use mail_domain::{
        AccountDriver, AccountSettings, AccountTransportSettings, MessageRecord, SmartMailbox,
        SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
        SmartMailboxId, SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule,
        SmartMailboxRuleNode, SmartMailboxValue, SyncCursor,
    };

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("mail-store-test-{now}-{seq}"))
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

    fn create_account(store: &DatabaseStore, account_id: &AccountId, name: &str) -> Result<(), StoreError> {
        store.create_account(&AccountSettings {
            id: account_id.clone(),
            name: name.to_string(),
            driver: AccountDriver::Mock,
            enabled: true,
            transport: AccountTransportSettings {
                base_url: None,
                username: None,
                secret_ref: None,
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        })
    }

    fn user_smart_mailbox(id: &str, name: &str, field: SmartMailboxField, value: SmartMailboxValue) -> SmartMailbox {
        SmartMailbox {
            id: SmartMailboxId::from(id),
            name: name.to_string(),
            position: 99,
            kind: SmartMailboxKind::User,
            default_key: None,
            parent_id: None,
            rule: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field,
                        operator: SmartMailboxOperator::Equals,
                        negated: false,
                        value,
                    })],
                },
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn account_scoped_reads_do_not_leak() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");
        create_account(&store, &account_a, "Primary")?;
        create_account(&store, &account_b, "Secondary")?;

        store.apply_sync_batch(
            &account_a,
            &SyncBatch {
                mailboxes: vec![mail_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message("shared-id", "inbox", Some("mime-a"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
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
                mailboxes: vec![mail_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message("shared-id", "inbox", Some("mime-b"))],
                deleted_mailbox_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
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
        create_account(&store, &account, "Primary")?;
        let result = store.apply_sync_batch(
            &account,
            &SyncBatch {
                mailboxes: vec![mail_domain::MailboxRecord {
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

        let first = store.append_event(&account, "message.updated", None, None, json!({"n": 1}))?;
        let _second =
            store.append_event(&account, "message.updated", None, None, json!({"n": 2}))?;

        let events = store.list_events(&EventFilter {
            account_id: Some(account),
            topic: Some("message.updated".to_string()),
            mailbox_id: None,
            after_seq: Some(first.seq),
        })?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["n"], 2);
        Ok(())
    }

    #[test]
    fn seeded_default_smart_mailboxes_are_available_in_sidebar() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;

        let smart_mailboxes = store.list_smart_mailboxes()?;
        let sidebar = store.get_sidebar()?;

        assert!(smart_mailboxes.iter().any(|mailbox| mailbox.id.as_str() == "default-inbox"));
        assert!(smart_mailboxes.iter().any(|mailbox| mailbox.id.as_str() == "default-all-mail"));
        assert_eq!(smart_mailboxes.len(), sidebar.smart_mailboxes.len());
        Ok(())
    }

    #[test]
    fn smart_mailbox_queries_messages_across_enabled_sources() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");
        create_account(&store, &account_a, "Primary")?;
        create_account(&store, &account_b, "Secondary")?;

        for account in [&account_a, &account_b] {
            store.apply_sync_batch(
                account,
                &SyncBatch {
                    mailboxes: vec![mail_domain::MailboxRecord {
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
                    cursors: vec![SyncCursor {
                        object_type: SyncObject::Message,
                        state: "state".to_string(),
                        updated_at: "2026-03-31T10:00:00Z".to_string(),
                    }],
                },
            )?;
        }

        let smart_mailbox = user_smart_mailbox(
            "sm-inbox",
            "Inbox across sources",
            SmartMailboxField::MailboxRole,
            SmartMailboxValue::String("inbox".to_string()),
        );
        store.create_smart_mailbox(&smart_mailbox)?;

        let messages = store.list_smart_mailbox_messages(&smart_mailbox.id)?;

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|message| message.source_id == account_a));
        assert!(messages.iter().any(|message| message.source_id == account_b));
        Ok(())
    }

    #[test]
    fn invalid_smart_mailbox_rule_is_rejected() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let smart_mailbox = SmartMailbox {
            id: SmartMailboxId::from("sm-bad"),
            name: "Broken".to_string(),
            position: 10,
            kind: SmartMailboxKind::User,
            default_key: None,
            parent_id: None,
            rule: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::IsRead,
                        operator: SmartMailboxOperator::Contains,
                        negated: false,
                        value: SmartMailboxValue::String("yes".to_string()),
                    })],
                },
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        let result = store.create_smart_mailbox(&smart_mailbox);

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn arrival_event_only_emits_for_new_mailbox_membership() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
        create_account(&store, &account, "Primary")?;
        let first_batch = SyncBatch {
            mailboxes: vec![
                mail_domain::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                mail_domain::MailboxRecord {
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
            .filter(|event| event.topic == "message.arrived")
            .collect();
        let second_arrivals: Vec<_> = second_events
            .iter()
            .filter(|event| event.topic == "message.arrived")
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
}
