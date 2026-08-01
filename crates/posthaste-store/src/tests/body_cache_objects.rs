use super::*;

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
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            body_html: Some("<p>Hello</p>".to_string()),
            body_text: Some("Hello".to_string()),
            attachments: Vec::new(),
            raw_mime: None,
            list_unsubscribe: None,
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
fn read_raw_message_returns_cached_raw_bytes() -> Result<(), StoreError> {
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

    // No raw cached yet.
    assert!(store.read_raw_message(&account, &message_id)?.is_none());

    let raw = "From: a@example.test\r\nSubject: Hi\r\n\r\nBody";
    store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            body_html: None,
            body_text: Some("Body".to_string()),
            attachments: Vec::new(),
            raw_mime: Some(raw.to_string()),
            list_unsubscribe: None,
        },
    )?;

    let cached = store
        .read_raw_message(&account, &message_id)?
        .expect("cached raw bytes");
    assert_eq!(cached, raw.as_bytes());
    Ok(())
}
