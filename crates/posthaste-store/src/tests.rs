use std::sync::atomic::{AtomicU64, Ordering};
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
        remote_blob_id: None,
        subject: Some("Hello".to_string()),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        to: Vec::new(),
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

// spec: docs/L1-sync#cache-object-parity
#[test]
fn sync_batch_creates_body_cache_object_for_metadata_only_message() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");

    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;

    let row = cache_object_row(&store, &account, &message_id)?.expect("cache object");
    assert_eq!(row.0, "wanted");
    assert_eq!(row.1, "body_only");
    assert_eq!(row.2, 0);
    assert_eq!(row.3, 0);
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        1
    );
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn sync_batch_marks_body_cache_object_cached_when_body_is_present() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");

    seed_messages(
        &store,
        &account,
        vec![sample_message(message_id.as_str(), "inbox", None)],
        "state-1",
    )?;

    let row = cache_object_row(&store, &account, &message_id)?.expect("cache object");
    assert_eq!(row.0, "cached");
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn apply_message_body_marks_body_cache_object_cached() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;

    let result = store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            body_html: Some("<p>Hello</p>".to_string()),
            body_text: Some("Hello".to_string()),
            attachments: Vec::new(),
            raw_mime: None,
        },
    )?;

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].topic, EVENT_TOPIC_MESSAGE_BODY_CACHED);
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account.clone()),
                topic: Some(EVENT_TOPIC_MESSAGE_BODY_CACHED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .len(),
        1
    );
    let row = cache_object_row(&store, &account, &message_id)?.expect("cache object");
    assert_eq!(row.0, "cached");
    Ok(())
}

#[test]
fn set_keywords_emits_keywords_changed_event_topic() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;
    let event_count_before = store
        .list_events(&EventFilter {
            account_id: Some(account.clone()),
            topic: Some(EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .len();

    let result = store.set_keywords(
        &account,
        &message_id,
        None,
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        },
    )?;

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].topic, EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED);
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .len(),
        event_count_before + 1
    );
    Ok(())
}

#[test]
fn replace_mailboxes_emits_mailboxes_changed_event_topic() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;
    let event_count_before = store
        .list_events(&EventFilter {
            account_id: Some(account.clone()),
            topic: Some(EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .len();

    let result = store.replace_mailboxes(
        &account,
        &message_id,
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;

    assert!(result
        .events
        .iter()
        .any(|event| event.topic == EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED));
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .len(),
        event_count_before + 1
    );
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn deleting_message_removes_cache_object_signals_and_rescore_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;
    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: account.to_string(),
        message_id: message_id.to_string(),
        reason: "search-visible".to_string(),
        search: None,
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    }])?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: vec![message_id.clone()],
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert!(store
        .list_events(&EventFilter {
            account_id: Some(account.clone()),
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .iter()
        .any(|event| event.message_id.as_ref() == Some(&message_id)
            && event
                .payload
                .get("deleted")
                .and_then(|value| value.as_bool())
                == Some(true)));
    assert_eq!(
        cache_child_count(&store, "cache_object", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_message_signal", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        0
    );
    Ok(())
}

#[test]
fn deleted_mailbox_removes_memberships_and_imap_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    seed_messages(
        &store,
        &account,
        vec![sample_message(message_id.as_str(), "archive", None)],
        "state-1",
    )?;
    let imap_mailbox_id = MailboxId::from("archive");
    let state = ImapMailboxSyncState::new(
        imap_mailbox_id.clone(),
        "Archive".to_string(),
        ImapUidValidity(10),
        "2026-04-25T00:00:00Z".to_string(),
    );
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: imap_mailbox_id.clone(),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    store.put_imap_mailbox_state(&account, &state)?;
    store.put_imap_message_location(&account, &location)?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: vec![imap_mailbox_id.clone()],
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        Vec::<MailboxId>::new()
    );
    assert!(store
        .get_imap_mailbox_state(&account, &imap_mailbox_id)?
        .is_none());
    assert_eq!(
        store.list_imap_mailbox_message_locations(&account, &imap_mailbox_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    assert!(store
        .list_events(&EventFilter {
            account_id: Some(account),
            topic: Some(EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .iter()
        .any(|event| event.message_id.as_ref() == Some(&message_id)));
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn opening_store_repairs_missing_body_cache_objects() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let data_root = root.join("data");
    let account = AccountId::from("primary");
    let message_id = MessageId::from("legacy-message");
    {
        let store = DatabaseStore::open(&db_path, &data_root)?;
        store.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO message (account_id, id, thread_id, received_at, size)
                     VALUES (?1, ?2, 'thread-1', '2026-04-27T00:00:00Z', 4096)",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })?;
    }

    let store = DatabaseStore::open(db_path, data_root)?;

    let row = cache_object_row(&store, &account, &message_id)?.expect("cache object");
    assert_eq!(row.0, "wanted");
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        1
    );
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn opening_store_prunes_orphan_cache_child_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let data_root = root.join("data");
    let account = AccountId::from("primary");
    let message_id = MessageId::from("orphan-message");
    {
        let _store = DatabaseStore::open(&db_path, &data_root)?;
    }
    {
        let connection =
            Connection::open(&db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .map_err(sql_to_store_error)?;
        connection
                .execute(
                    "INSERT INTO cache_object (
                        account_id, message_id, layer, object_id, fetch_unit, state,
                        value_bytes, fetch_bytes, priority, reason, last_scored_at
                     ) VALUES (?1, ?2, 'body', '', 'body_only', 'wanted', 0, 4096, 1, 'legacy', '2026-04-27T00:00:00Z')",
                    params![account.as_str(), message_id.as_str()],
                )
                .map_err(sql_to_store_error)?;
        connection
            .execute(
                "INSERT INTO cache_message_signal (
                        account_id, message_id, direct_user_boost, dirty_at
                     ) VALUES (?1, ?2, 1, '2026-04-27T00:00:00Z')",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
        connection
            .execute(
                "INSERT INTO cache_rescore_queue (account_id, message_id, reason, queued_at)
                     VALUES (?1, ?2, 'legacy', '2026-04-27T00:00:00Z')",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
    }

    let store = DatabaseStore::open(db_path, data_root)?;

    assert_eq!(
        cache_child_count(&store, "cache_object", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_message_signal", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        0
    );
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
fn sender_address_cache_upserts_by_account_and_normalized_email() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");

    store.remember_sender_address(
        &primary,
        &Recipient {
            name: Some("Catch One".to_string()),
            email: "Catch@Example.test".to_string(),
        },
    )?;
    store.remember_sender_address(
        &primary,
        &Recipient {
            name: Some("Catch Two".to_string()),
            email: "catch@example.test".to_string(),
        },
    )?;
    store.remember_sender_address(
        &secondary,
        &Recipient {
            name: None,
            email: "catch@example.test".to_string(),
        },
    )?;

    let cached = store.list_sender_address_cache()?;

    assert_eq!(cached.len(), 2);
    assert!(cached.iter().any(|sender| {
        sender.source_id == primary
            && sender.name.as_deref() == Some("Catch Two")
            && sender.email == "catch@example.test"
    }));
    assert!(cached.iter().any(|sender| {
        sender.source_id == secondary
            && sender.name.is_none()
            && sender.email == "catch@example.test"
    }));
    Ok(())
}

#[test]
fn sender_address_cache_ignores_non_concrete_sender_addresses() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    for email in [
        "*@example.test",
        "missing-at",
        "a b@example.test",
        "@example.test",
    ] {
        store.remember_sender_address(
            &account,
            &Recipient {
                name: None,
                email: email.to_string(),
            },
        )?;
    }

    assert!(store.list_sender_address_cache()?.is_empty());
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

#[test]
fn imap_message_locations_round_trip_multiple_mailboxes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("imap:gmail:msgid:1278455344230334865");
    let inbox = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: Some(ImapModSeq(u64::MAX)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let all_mail = ImapMessageLocation {
        mailbox_id: MailboxId::from("imap:all"),
        uid: ImapUid(202),
        ..inbox.clone()
    };

    store.put_imap_message_location(&account, &all_mail)?;
    store.put_imap_message_location(&account, &inbox)?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![all_mail, inbox.clone()]
    );
    assert_eq!(
        store.list_imap_mailbox_message_locations(&account, &MailboxId::from("imap:inbox"))?,
        vec![inbox]
    );
    Ok(())
}

#[test]
fn imap_message_location_delete_is_account_scoped() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");
    let message_id = MessageId::from("message-1");
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    store.put_imap_message_location(&primary, &location)?;
    store.put_imap_message_location(&secondary, &location)?;
    store.delete_imap_message_locations(&primary, &message_id)?;

    assert_eq!(
        store.list_imap_message_locations(&primary, &message_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    assert_eq!(
        store.list_imap_message_locations(&secondary, &message_id)?,
        vec![location]
    );
    Ok(())
}

#[test]
fn delete_source_data_removes_imap_state_and_locations() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    let mailbox_id = MailboxId::from("imap:inbox");
    let state = ImapMailboxSyncState::new(
        mailbox_id.clone(),
        "INBOX".to_string(),
        ImapUidValidity(10),
        "2026-04-25T00:00:00Z".to_string(),
    );
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: mailbox_id.clone(),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    store.put_imap_mailbox_state(&account, &state)?;
    store.put_imap_message_location(&account, &location)?;
    store.delete_source_data(&account)?;

    assert!(store
        .get_imap_mailbox_state(&account, &mailbox_id)?
        .is_none());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        Vec::<ImapMessageLocation>::new()
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
fn sync_batch_persists_and_deletes_imap_message_locations() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("message-1");
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: Some(ImapModSeq(999)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let mailbox_state = ImapMailboxSyncState {
        mailbox_id: MailboxId::from("imap:inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(10),
        highest_uid: Some(ImapUid(101)),
        highest_modseq: Some(ImapModSeq(999)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

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
            imap_mailbox_states: vec![mailbox_state.clone()],
            imap_message_locations: vec![location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![location]
    );
    assert_eq!(
        store.get_imap_mailbox_state(&account, &MailboxId::from("imap:inbox"))?,
        Some(mailbox_state)
    );

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: vec![message_id.clone()],
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    Ok(())
}

// spec: docs/L0-testing#store-reconciliation-contracts
// spec: docs/L1-sync#imap-locations
// spec: docs/L1-sync#message-snapshot-authoritative
// spec: docs/L1-sync#gmail-label-canonicalization
#[test]
fn full_imap_snapshot_prunes_stale_location_without_deleting_canonical_message(
) -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("imap:gmail:rfc822msgid:canonical");
    let sent_id = MailboxId::from("imap:sent");
    let starred_id = MailboxId::from("imap:starred");
    let sent_mailbox = posthaste_domain::MailboxRecord {
        id: sent_id.clone(),
        name: "Sent".to_string(),
        role: Some("sent".to_string()),
        unread_emails: 0,
        total_emails: 0,
    };
    let starred_mailbox = posthaste_domain::MailboxRecord {
        id: starred_id.clone(),
        name: "Starred".to_string(),
        role: None,
        unread_emails: 0,
        total_emails: 0,
    };
    let sent_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: sent_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(12),
        modseq: Some(ImapModSeq(90)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let starred_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: starred_id.clone(),
        uid_validity: ImapUidValidity(8),
        uid: ImapUid(44),
        modseq: Some(ImapModSeq(91)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let canonical_message = MessageRecord {
        id: message_id.clone(),
        mailbox_ids: vec![sent_id.clone(), starred_id],
        keywords: vec!["$flagged".to_string(), "$seen".to_string()],
        ..sample_message(message_id.as_str(), sent_id.as_str(), Some("mime"))
    };

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![sent_mailbox.clone(), starred_mailbox.clone()],
            messages: vec![canonical_message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![sent_location.clone(), starred_location],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-1", "2026-04-25T00:00:00Z")],
        },
    )?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![sent_mailbox, starred_mailbox],
            messages: vec![MessageRecord {
                mailbox_ids: vec![sent_id],
                keywords: vec!["$seen".to_string()],
                ..sample_message(message_id.as_str(), "imap:sent", Some("mime"))
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![sent_location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![message_cursor("message-2", "2026-04-25T00:05:00Z")],
        },
    )?;

    assert!(store.get_message_detail(&account, &message_id)?.is_some());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![sent_location]
    );
    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        vec![MailboxId::from("imap:sent")]
    );
    Ok(())
}

// spec: docs/L0-testing#store-reconciliation-contracts
// spec: docs/L1-sync#syncbatch-and-apply_sync_batch
#[test]
fn partial_imap_location_delete_removes_only_that_mailbox_membership() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    let message_id = MessageId::from("imap:gmail:rfc822msgid:canonical");
    let archive_id = MailboxId::from("imap:archive");
    let inbox_id = MailboxId::from("imap:inbox");
    let archive_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: archive_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(12),
        modseq: Some(ImapModSeq(90)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let inbox_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: inbox_id.clone(),
        uid_validity: ImapUidValidity(8),
        uid: ImapUid(44),
        modseq: Some(ImapModSeq(91)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let message = MessageRecord {
        id: message_id.clone(),
        mailbox_ids: vec![archive_id.clone(), inbox_id.clone()],
        ..sample_message(message_id.as_str(), archive_id.as_str(), Some("mime"))
    };

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: archive_id.clone(),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain::MailboxRecord {
                    id: inbox_id.clone(),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
            ],
            messages: vec![message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: vec![archive_location.clone(), inbox_location.clone()],
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: Vec::new(),
        },
    )?;

    let events = store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: vec![inbox_location.key()],
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert!(store.get_message_detail(&account, &message_id)?.is_some());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![archive_location]
    );
    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        vec![archive_id]
    );
    assert!(events
        .iter()
        .any(|event| event.topic == EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED));
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED.to_string()),
                mailbox_id: Some(inbox_id),
                after_seq: None,
            })?
            .len(),
        1
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
fn message_detail_preserves_recipients() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut message = sample_message("sent-message", "sent", Some("mime"));
    message.to = vec![Recipient {
        name: Some("Bob Recipient".to_string()),
        email: "bob@example.com".to_string(),
    }];
    seed_messages(&store, &account, vec![message], "state-1")?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("sent-message"))?
        .unwrap();
    assert_eq!(detail.summary.to.len(), 1);
    assert_eq!(detail.summary.to[0].name.as_deref(), Some("Bob Recipient"));
    assert_eq!(detail.summary.to[0].email, "bob@example.com");
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
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
fn bulk_message_hydration_preserves_order_and_account_scoped_metadata() -> Result<(), StoreError> {
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
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
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
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
fn replace_mailboxes_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
fn mailbox_role_override_survives_full_mailbox_snapshot() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let batch = SyncBatch {
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
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: Vec::new(),
    };

    store.apply_sync_batch(&account, &batch)?;
    store.set_mailbox_role_override(
        &account,
        &MailboxId::from("all-mail"),
        Some("archive"),
        None,
    )?;
    store.apply_sync_batch(&account, &batch)?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "all-mail")
            .and_then(|mailbox| mailbox.role.as_deref()),
        Some("archive")
    );
    Ok(())
}

#[test]
fn mailbox_role_override_can_clear_discovered_previous_owner() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let batch = SyncBatch {
        mailboxes: vec![
            posthaste_domain::MailboxRecord {
                id: MailboxId::from("server-archive"),
                name: "Archive".to_string(),
                role: Some("archive".to_string()),
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
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: Vec::new(),
    };

    store.apply_sync_batch(&account, &batch)?;
    store.set_mailbox_role_override(
        &account,
        &MailboxId::from("all-mail"),
        Some("archive"),
        Some(&MailboxId::from("server-archive")),
    )?;
    store.apply_sync_batch(&account, &batch)?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "server-archive")
            .and_then(|mailbox| mailbox.role.as_deref()),
        None
    );
    assert_eq!(
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id.as_str() == "all-mail")
            .and_then(|mailbox| mailbox.role.as_deref()),
        Some("archive")
    );
    Ok(())
}

#[test]
fn mailbox_role_override_rejects_duplicate_role_without_clear() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain::MailboxRecord {
                    id: MailboxId::from("server-archive"),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    let error = store
        .set_mailbox_role_override(
            &account,
            &MailboxId::from("all-mail"),
            Some("archive"),
            None,
        )
        .expect_err("duplicate role should be rejected");

    assert!(matches!(error, StoreError::Conflict(message) if message.contains("archive")));
    Ok(())
}

#[test]
fn mailbox_role_override_rejects_unsupported_role() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let error = store
        .set_mailbox_role_override(
            &account,
            &MailboxId::from("all-mail"),
            Some("important"),
            None,
        )
        .expect_err("unsupported role should be rejected");

    assert!(matches!(error, StoreError::Conflict(message) if message.contains("important")));
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
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
