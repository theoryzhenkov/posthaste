use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use hex::encode as hex_encode;
use mail_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    CommandResult, DomainEvent, EventFilter, FetchedBody, MailStore, MailboxId, MailboxSummary,
    MessageDetail, MessageId, MessageSummary, RawMessageRef, ReplaceMailboxesCommand,
    SetKeywordsCommand, StoreError, SyncBatch, SyncCursor, SyncObject, ThreadId, ThreadView,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
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
                "SELECT id, thread_id, subject, from_name, from_email, preview,
                        received_at, has_attachment, is_read, is_flagged
                 FROM message
                 WHERE account_id = ?1 AND thread_id = ?2
                 ORDER BY received_at ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str(), thread_id.as_str()], |row| {
                row_to_message_summary(&connection, account_id, row)
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

    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let sql = if mailbox_id.is_some() {
            "SELECT m.id, m.thread_id, m.subject, m.from_name, m.from_email, m.preview,
                    m.received_at, m.has_attachment, m.is_read, m.is_flagged
             FROM message m
             JOIN message_mailbox mm
               ON mm.account_id = m.account_id
              AND mm.message_id = m.id
             WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
             ORDER BY m.received_at DESC"
        } else {
            "SELECT id, thread_id, subject, from_name, from_email, preview,
                    received_at, has_attachment, is_read, is_flagged
             FROM message
             WHERE account_id = ?1
             ORDER BY received_at DESC"
        };
        let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
        let summaries = if let Some(mailbox_id) = mailbox_id {
            statement
                .query_map(params![account_id.as_str(), mailbox_id.as_str()], |row| {
                    row_to_message_summary(&connection, account_id, row)
                })
                .map_err(sql_to_store_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_to_store_error)?
        } else {
            statement
                .query_map(params![account_id.as_str()], |row| {
                    row_to_message_summary(&connection, account_id, row)
                })
                .map_err(sql_to_store_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_to_store_error)?
        };
        Ok(summaries)
    }

    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, thread_id, subject, from_name, from_email, preview,
                        received_at, has_attachment, is_read, is_flagged
                 FROM message
                 WHERE account_id = ?1 AND id = ?2",
            )
            .map_err(sql_to_store_error)?;

        let summary = statement
            .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
                row_to_message_summary(&connection, account_id, row)
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
                delete_message_tx(tx, account_id, message_id)?;
                for mailbox_id in prior_mailboxes {
                    affected_mailboxes.insert(mailbox_id);
                }
                if let Some(thread_id) = thread_id {
                    affected_threads.insert(thread_id);
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
                let existed = tx
                    .query_row(
                        "SELECT 1 FROM message WHERE account_id = ?1 AND id = ?2",
                        params![account_id.as_str(), message.id.as_str()],
                        |_row| Ok(()),
                    )
                    .optional()
                    .map_err(sql_to_store_error)?
                    .is_some();

                tx.execute(
                    "INSERT INTO message (
                        account_id, id, thread_id, remote_blob_id, subject, from_name, from_email,
                        preview, received_at, has_attachment, size, is_read, is_flagged
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT(account_id, id) DO UPDATE SET
                        thread_id = excluded.thread_id,
                        remote_blob_id = excluded.remote_blob_id,
                        subject = excluded.subject,
                        from_name = excluded.from_name,
                        from_email = excluded.from_email,
                        preview = excluded.preview,
                        received_at = excluded.received_at,
                        has_attachment = excluded.has_attachment,
                        size = excluded.size,
                        is_read = excluded.is_read,
                        is_flagged = excluded.is_flagged",
                    params![
                        account_id.as_str(),
                        message.id.as_str(),
                        message.thread_id.as_str(),
                        message.remote_blob_id.as_ref().map(|blob_id| blob_id.as_str()),
                        message.subject,
                        message.from_name,
                        message.from_email,
                        message.preview,
                        message.received_at,
                        bool_to_i64(message.has_attachment),
                        message.size,
                        bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$seen")),
                        bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$flagged"))
                    ],
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

                affected_threads.insert(message.thread_id.clone());
                for mailbox_id in previous_mailboxes.iter().chain(message.mailbox_ids.iter()) {
                    affected_mailboxes.insert(mailbox_id.clone());
                }

                events.push(insert_event_tx(
                    tx,
                    account_id,
                    "message.updated",
                    message.mailbox_ids.first(),
                    Some(&message.id),
                    json!({ "messageId": message.id.as_str(), "threadId": message.thread_id.as_str(), "created": !existed }),
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
                remote_blob_id TEXT,
                subject TEXT,
                from_name TEXT,
                from_email TEXT,
                preview TEXT,
                received_at TEXT NOT NULL,
                has_attachment INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                is_read INTEGER NOT NULL DEFAULT 1,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, id)
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
            CREATE INDEX IF NOT EXISTS idx_message_mailbox
                ON message_mailbox (account_id, mailbox_id);
            CREATE INDEX IF NOT EXISTS idx_message_keyword
                ON message_keyword (account_id, keyword);
            CREATE INDEX IF NOT EXISTS idx_event_log_lookup
                ON event_log (account_id, topic, mailbox_id, seq);
            ",
        )
        .map_err(sql_to_store_error)
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
    account_id: &AccountId,
    row: &rusqlite::Row<'_>,
) -> Result<MessageSummary, rusqlite::Error> {
    let message_id = MessageId(row.get(0)?);
    let mailbox_ids =
        fetch_mailbox_ids(connection, account_id, &message_id).map_err(store_to_sqlite_error)?;
    let keywords =
        fetch_keywords(connection, account_id, &message_id).map_err(store_to_sqlite_error)?;
    Ok(MessageSummary {
        id: message_id.clone(),
        thread_id: ThreadId(row.get(1)?),
        subject: row.get(2)?,
        from_name: row.get(3)?,
        from_email: row.get(4)?,
        preview: row.get(5)?,
        received_at: row.get(6)?,
        has_attachment: row.get::<_, i64>(7)? != 0,
        is_read: row.get::<_, i64>(8)? != 0,
        is_flagged: row.get::<_, i64>(9)? != 0,
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
            "SELECT id, thread_id, subject, from_name, from_email, preview,
                    received_at, has_attachment, is_read, is_flagged
             FROM message
             WHERE account_id = ?1 AND id = ?2",
        )
        .map_err(sql_to_store_error)?;

    let detail = statement
        .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
            let summary = MessageSummary {
                id: MessageId(row.get(0)?),
                thread_id: ThreadId(row.get(1)?),
                subject: row.get(2)?,
                from_name: row.get(3)?,
                from_email: row.get(4)?,
                preview: row.get(5)?,
                received_at: row.get(6)?,
                has_attachment: row.get::<_, i64>(7)? != 0,
                is_read: row.get::<_, i64>(8)? != 0,
                is_flagged: row.get::<_, i64>(9)? != 0,
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

    use mail_domain::{MessageRecord, SyncCursor};

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
            thread_id: ThreadId::from("thread-1"),
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
        }
    }

    #[test]
    fn account_scoped_reads_do_not_leak() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account_a = AccountId::from("primary");
        let account_b = AccountId::from("secondary");

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
    fn arrival_event_only_emits_for_new_mailbox_membership() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
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
