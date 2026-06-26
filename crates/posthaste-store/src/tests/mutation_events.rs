use super::*;

#[test]
fn set_keywords_emits_message_updated_keyword_change() -> Result<(), StoreError> {
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
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
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
    assert_eq!(result.events[0].topic, EVENT_TOPIC_MESSAGE_UPDATED);
    assert_eq!(result.events[0].payload["changes"]["keywords"], true);
    assert_eq!(
        result.events[0].payload["assertion"]["after"]["id"],
        message_id.as_str()
    );
    assert_eq!(
        result.events[0].payload["assertion"]["after"]["keywords"],
        serde_json::json!(["$flagged", "$seen"])
    );
    assert_eq!(
        result.events[0].payload["assertion"]["after"]["conversationRef"]["conversationId"],
        serde_json::json!(result
            .detail
            .as_ref()
            .unwrap()
            .summary
            .conversation_id
            .as_str())
    );
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .len(),
        event_count_before + 1
    );
    Ok(())
}

#[test]
fn replace_mailboxes_emits_message_updated_mailbox_change() -> Result<(), StoreError> {
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
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
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

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].topic, EVENT_TOPIC_MESSAGE_UPDATED);
    assert_eq!(result.events[0].payload["changes"]["mailboxes"], true);
    assert_eq!(result.events[0].payload["changes"]["arrived"], true);
    assert_eq!(
        result.events[0].payload["arrivedMailboxIds"],
        serde_json::json!(["archive"])
    );
    assert_eq!(
        store
            .list_events(&EventFilter {
                account_id: Some(account),
                topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
                mailbox_id: None,
                after_seq: None,
            })?
            .len(),
        event_count_before + 1
    );
    Ok(())
}

#[test]
fn command_message_updated_carries_projection_matching_served_summary() -> Result<(), StoreError> {
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

    let result = store.set_keywords(
        &account,
        &message_id,
        None,
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        },
    )?;

    let projection = &result.events[0].payload["projection"];
    let served = serde_json::to_value(&result.detail.as_ref().unwrap().summary).unwrap();
    // The event-promoted projection is byte-identical to the served summary —
    // one derivation (no second projection path). The store reads these fields
    // for sort key / row key / membership / fold / render.
    assert_eq!(projection["receivedAt"], served["receivedAt"]);
    assert_eq!(projection["sourceId"], served["sourceId"]);
    assert_eq!(projection["mailboxIds"], served["mailboxIds"]);
    assert_eq!(projection["keywords"], served["keywords"]);
    assert_eq!(projection["isFlagged"], served["isFlagged"]);
    assert_eq!(*projection, served, "full projection byte-equals served");
    Ok(())
}

#[test]
fn sync_message_updated_carries_projection() -> Result<(), StoreError> {
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

    let events = store.list_events(&EventFilter {
        account_id: Some(account),
        topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
        mailbox_id: None,
        after_seq: None,
    })?;
    let event = events
        .iter()
        .find(|event| event.payload["messageId"] == message_id.as_str())
        .expect("message.updated for message-1");
    // The sync path attaches the same `projection` the command path does, so
    // the store can materialize a promoted never-held message from either.
    let projection = &event.payload["projection"];
    assert_eq!(projection["sourceId"], "primary");
    assert_eq!(projection["mailboxIds"], serde_json::json!(["inbox"]));
    assert!(projection["receivedAt"].as_str().is_some());
    Ok(())
}

#[test]
fn message_updated_carries_count_deltas_matching_served_counts() -> Result<(), StoreError> {
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

    // Move the message inbox -> archive: both mailboxes' counts change.
    let result = store.replace_mailboxes(
        &account,
        &message_id,
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;

    let count_deltas = &result.events[0].payload["countDeltas"];
    assert!(count_deltas.is_array(), "countDeltas attached to the event");
    // Each delta matches the served mailbox count — one derivation (no second
    // count path). The store reads these as `mailbox[id].count`.
    let served: std::collections::HashMap<String, (i64, i64)> = store
        .list_mailboxes(&account)?
        .into_iter()
        .map(|m| (m.id.as_str().to_string(), (m.unread_emails, m.total_emails)))
        .collect();
    let by_id: std::collections::HashMap<&str, &serde_json::Value> = count_deltas
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["mailboxId"].as_str().unwrap(), d))
        .collect();
    for (id, delta) in &by_id {
        let (unread, total) = served[*id];
        assert_eq!(delta["unreadCount"], unread, "unreadCount matches served for {id}");
        assert_eq!(delta["totalCount"], total, "totalCount matches served for {id}");
    }
    // The move is reflected: inbox lost the message (total 0), archive gained it.
    assert_eq!(by_id["inbox"]["totalCount"], 0, "inbox total dropped to 0");
    assert_eq!(by_id["archive"]["totalCount"], 1, "archive total rose to 1");
    Ok(())
}
