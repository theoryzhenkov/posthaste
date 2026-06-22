use super::*;

#[tokio::test]
async fn archive_assertion_moves_message_in_read_overlay_without_projection_write() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive assertion queues");

    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("projection mailbox lookup"),
        vec![MailboxId::from("inbox")],
        "authoritative projection remains provider-owned",
    );
    assert!(service
        .list_messages(&account, Some(&MailboxId::from("inbox")))
        .expect("inbox overlay read")
        .is_empty(),);
    let archive = service
        .list_messages(&account, Some(&MailboxId::from("archive")))
        .expect("archive overlay read");
    assert_eq!(archive.len(), 1);
    assert_eq!(archive[0].id, MessageId::from("message-1"));
}

#[tokio::test]
async fn archive_assertion_appears_in_rule_page_query_path() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store, config);

    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive assertion queues");

    let archive_rule = SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes: vec![
                SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::SourceId,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("primary".to_string()),
                }),
                SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::MailboxId,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("archive".to_string()),
                }),
            ],
        },
    };

    let page = service
        .query_message_page_by_rule(
            &archive_rule,
            50,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .expect("rule query page should fold overlay");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, MessageId::from("message-1"));
}

#[tokio::test]
async fn archive_assertion_appears_in_role_based_rule_query_path() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store, config);

    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive assertion queues");

    let archive_role_rule = SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxRole,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String("archive".to_string()),
            })],
        },
    };

    let page = service
        .query_message_page_by_rule(
            &archive_role_rule,
            50,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .expect("role query page should fold overlay");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, MessageId::from("message-1"));
}

#[tokio::test]
async fn replace_mailboxes_coalesces_pending_move() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    for mailbox in ["archive", "junk"] {
        service
            .replace_mailboxes(
                &account,
                &MessageId::from("message-1"),
                &ReplaceMailboxesCommand {
                    mailbox_ids: vec![MailboxId::from(mailbox)],
                },
            )
            .await
            .expect("move queues");
    }

    let pending = service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 1, "repeated moves coalesce to the latest");
    let command = serde_json::from_value::<ReplaceMailboxesCommand>(pending[0].payload.clone())
        .expect("payload");
    assert_eq!(command.mailbox_ids, vec![MailboxId::from("junk")]);
}

#[tokio::test]
async fn destroy_supersedes_pending_assertions() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let service = MailService::new(store, Arc::new(TestConfig::default()));

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
        .expect("flag queues");
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("move queues");
    service
        .destroy_message(&account, &MessageId::from("message-1"))
        .await
        .expect("destroy queues");

    let pending = service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 1, "destroy supersedes earlier assertions");
    assert_eq!(pending[0].kind, OperationKind::Destroy);
}

#[tokio::test]
async fn sidebar_and_smart_mailbox_counts_reflect_pending_archive() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let config = Arc::new(TestConfig {
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store, config);

    let inbox_rule = SmartMailboxRule {
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

    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive queues");

    let mailboxes = service.list_mailboxes(&account).expect("sidebar list");
    let inbox = mailboxes
        .iter()
        .find(|mailbox| mailbox.id == MailboxId::from("inbox"))
        .expect("inbox present");
    let archive = mailboxes
        .iter()
        .find(|mailbox| mailbox.id == MailboxId::from("archive"))
        .expect("archive present");
    assert_eq!((inbox.unread_emails, inbox.total_emails), (0, 0));
    assert_eq!((archive.unread_emails, archive.total_emails), (1, 1));
    assert_eq!(
        service
            .count_messages_by_rule(&inbox_rule)
            .expect("inbox count"),
        (0, 0),
        "smart-mailbox count folds the pending move out of inbox",
    );
}

#[tokio::test]
async fn mixed_message_mutations_apply_locally_without_chaining_assertions() {
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
        vec![MailboxId::from("inbox")],
        "local-first assertions do not mutate the authoritative projection",
    );
    let pending = service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].kind, OperationKind::SetKeywords);
    assert_eq!(pending[1].kind, OperationKind::ReplaceMailboxes);
    assert!(pending.iter().all(|op| op.depends_on.is_none()));
}

#[tokio::test]
async fn stale_local_cursor_does_not_conflict_message_assertion_flush() {
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
        .expect("flush should apply without an OCC base");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, EVENT_TOPIC_OPERATION_SETTLED);
    assert!(service
        .list_pending_operations(&account_id)
        .expect("pending operations should list")
        .is_empty());
    assert_eq!(
        store
            .get_cursor(&account_id, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-1",
        "flush does not advance local sync cursor; sync reconciles it",
    );
}

#[tokio::test]
async fn successful_flush_prunes_message_operation_without_advancing_local_cursor() {
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
