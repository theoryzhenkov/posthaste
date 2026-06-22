use super::*;

#[tokio::test]
async fn mixed_message_mutations_apply_locally_and_queue_in_order() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);

    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("first mutation should apply locally");
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("second mutation should apply locally");

    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-1",
        "provider cursor advances only by sync, not local optimistic apply",
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")]
    );
    let pending = service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].kind, OperationKind::SetKeywords);
    assert_eq!(pending[1].kind, OperationKind::ReplaceMailboxes);
    assert_eq!(pending[1].depends_on.as_ref(), Some(&pending[0].id));
}

#[tokio::test]
async fn state_mismatch_conflicts_during_outbox_flush_without_retrying_original_mutation() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(2);

    service
        .set_keywords(
            &account_id,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("stale mutation still applies locally and queues");

    let events = service
        .flush_account(&account_id, &gateway)
        .await
        .expect("flush should settle the conflict");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, EVENT_TOPIC_OPERATION_SETTLED);
    let pending = service
        .list_pending_operations(&account_id)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, OperationState::Conflicted);
    assert_eq!(pending[0].attempts, 1);
    assert_eq!(
        pending[0].last_error.as_deref(),
        Some("provider state diverged")
    );
    assert_eq!(
        store
            .get_cursor(&account_id, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-1",
    );
}

#[tokio::test]
async fn successful_flush_prunes_message_operation_and_calls_gateway_with_base_cursor() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_revision(1);

    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("mutation should queue");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    assert_eq!(events.len(), 1);
    assert!(service
        .list_pending_operations(&account)
        .expect("pending operations should list")
        .is_empty());
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-1",
        "flush does not advance the local cursor; follow-up sync reconciles it",
    );
}
