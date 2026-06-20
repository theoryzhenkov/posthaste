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
