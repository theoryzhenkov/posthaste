use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_domain::{
    search, MessageRecord, Recipient, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
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
        subject: Some("Hello".to_string()),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        preview: Some("Preview".to_string()),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from(account_mailbox)],
        keywords: vec!["$seen".to_string()],
        body_html: Some("<p>Hello</p>".to_string()),
        body_text: Some("Hello".to_string()),
        raw_mime: raw_mime.map(str::to_string),
        rfc_message_id: Some(format!("<{message_id}@example.test>")),
        ..Default::default()
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![message_cursor(cursor_state, "2026-03-31T10:00:00Z")],
        },
    )?;
    Ok(())
}

fn metadata_only_message(message_id: &str, account_mailbox: &str) -> MessageRecord {
    let mut message = sample_message(message_id, account_mailbox, None);
    message.body_html = None;
    message.body_text = None;
    message.raw_mime = None;
    message.size = 8 * 1024;
    message
}

fn cache_object_row(
    store: &DatabaseStore,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<(String, String, i64, i64)>, StoreError> {
    let connection = store.read_connection()?;
    connection
        .query_row(
            "SELECT state, fetch_unit, value_bytes, fetch_bytes
                 FROM cache_object
                 WHERE account_id = ?1
                   AND message_id = ?2
                   AND layer = 'body'
                   AND object_id = ''",
            params![account_id.as_str(), message_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(sql_to_store_error)
}

fn cache_child_count(
    store: &DatabaseStore,
    table: &str,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<i64, StoreError> {
    let connection = store.read_connection()?;
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE account_id = ?1 AND message_id = ?2");
    connection
        .query_row(
            &sql,
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)
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

mod body_cache_objects;
mod cache_cleanup;
mod conversation_queries;
mod conversation_threads_events;
mod fts_search;
mod imap_snapshots;
mod imap_state_locations;
mod mailbox_role_overrides;
mod mailbox_snapshots;
mod message_queries;
mod message_snapshots;
mod mutation_cursors;
mod mutation_events;
mod outbox;
mod reads_events_integrity;
mod reconcile;
mod repair;
mod rev_log;
mod smart_mailboxes;
mod snooze;
mod source_visibility;
mod tags_and_locations;

#[test]
fn write_transaction_recovers_from_poisoned_mutex() {
    let root = temp_root();
    let store = Arc::new(DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap());

    // Poison the write-connection mutex by panicking inside a transaction.
    let poisoner = Arc::clone(&store);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        poisoner
            .write_transaction(|_tx| -> Result<(), StoreError> {
                panic!("intentional test panic while holding write lock");
            })
            .ok();
    }));
    assert!(result.is_err(), "panic inside operation should propagate");

    // The next write transaction must succeed: the mutex should have been
    // recovered rather than left poisoned forever.
    store
        .write_transaction(|tx: &rusqlite::Transaction<'_>| {
            tx.query_row("SELECT 1", [], |_| Ok(()))
                .map_err(|err| StoreError::Failure(err.to_string()))?;
            Ok(())
        })
        .expect("store should recover from a poisoned write mutex");
}
