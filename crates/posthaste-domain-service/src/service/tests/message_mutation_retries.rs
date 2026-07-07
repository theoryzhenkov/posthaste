use super::*;

#[tokio::test]
async fn archive_mutation_writes_through_to_canonical_projection() {
    // S2 write-through: a message mutation applies its assertion to the canonical
    // projection immediately (no longer overlay-only). That reads then reflect it
    // is a store property (the indexed query/triggers read canonical) covered in
    // posthaste-store; TestStore decouples its rule/list fixtures from the write
    // path, so here we assert the write-through reaches the projection.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
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
        .expect("archive assertion applies");

    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("projection mailbox lookup"),
        vec![MailboxId::from("archive")],
        "the mutation writes its assertion through to the canonical projection",
    );
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

    let archive_rule = MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes: vec![
                MailQueryRuleNode::Condition(MailQueryCondition {
                    field: MailQueryField::SourceId,
                    operator: MailQueryOperator::Equals,
                    negated: false,
                    value: MailQueryValue::String("primary".to_string()),
                }),
                MailQueryRuleNode::Condition(MailQueryCondition {
                    field: MailQueryField::MailboxId,
                    operator: MailQueryOperator::Equals,
                    negated: false,
                    value: MailQueryValue::String("archive".to_string()),
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

    let archive_role_rule = MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes: vec![MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::MailboxRole,
                operator: MailQueryOperator::Equals,
                negated: false,
                value: MailQueryValue::String("archive".to_string()),
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
async fn mixed_message_mutations_write_through_to_projection() {
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
        "provider cursor advances only by sync, not local optimistic write-through",
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")],
        "both assertions write through to the canonical projection (last one wins on mailbox)",
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
async fn get_conversation_reads_canonical_without_overlay_fold() {
    // S4: get_conversation returns the canonical conversation view directly — it
    // no longer folds the outbox overlay over it. Optimism is written through to
    // canonical, so the conversation_reader already reflects pending assertions
    // in production; here (TestStore's conversation_view is a fixture decoupled
    // from the write path) we assert get_conversation returns it verbatim.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store
        .conversation_view
        .lock()
        .expect("conversation view lock") = Some(ConversationView {
        id: ConversationId::from("conv-1"),
        subject: Some("Hi".to_string()),
        messages: vec![sample_message_summary("message-1", Vec::new())],
    });
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    // A pending archive does not fold into the read (no overlay); the view
    // reflects whatever canonical holds.
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive assertion applies");

    let view = service
        .get_conversation(&ConversationId::from("conv-1"))
        .expect("conversation reads");
    assert_eq!(view.messages.len(), 1);
    assert_eq!(
        view.messages[0].id,
        MessageId::from("message-1"),
        "get_conversation returns the canonical view verbatim — no overlay fold",
    );
}

#[tokio::test]
async fn flush_settles_message_assertion_from_readback_and_removes_it() {
    // S2: a flushed message assertion settles from the provider readback (set+get)
    // and is removed at flush — it no longer rests in `applied` awaiting a sync.
    // The readback is the new base; remaining unsettled ops fold over it; canonical
    // reflects the result.
    //
    // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    // The provider applied the archive: set+get reads the message back in archive.
    let mut readback = sample_message_record("message-1", 0, false);
    readback.mailbox_ids = vec![MailboxId::from("archive")];
    gateway
        .readbacks
        .lock()
        .expect("readbacks lock poisoned")
        .push(posthaste_domain_model::MessageReadback::Present(readback));

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

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, EVENT_TOPIC_OPERATION_SETTLED);

    // Settled and removed at flush — nothing rests in applied or pending.
    assert!(service
        .list_pending_operations(&account)
        .expect("pending list")
        .is_empty());

    // Canonical reflects the settled readback.
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("projection lookup"),
        vec![MailboxId::from("archive")],
        "settle wrote the provider readback to canonical",
    );
}

/// A one-message observe batch that reports `record` as provider truth, plus a
/// fresh message cursor so the test store advances.
fn observe_batch(record: MessageRecord) -> SyncBatch {
    SyncBatch {
        mailboxes: Vec::new(),
        messages: vec![record],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Message,
            state: "message-2".to_string(),
            updated_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
        }],
    }
}

#[tokio::test]
async fn sync_flushes_and_settles_the_pending_assertion() {
    // S2: `sync_account` flushes the outbox after observing, so a pending
    // assertion settles from the readback and is removed — settlement rides the
    // flush whether triggered directly or by a sync's post-flush. (The old
    // rest-in-applied/no-premature-retire mechanism this test used is gone.)
    //
    // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    service
        .replace_mailboxes(
            &account_id,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("archive assertion queues");

    // The provider applied the archive; the observe reflects it.
    let mut record = sample_message_record("message-1", 0, false);
    record.mailbox_ids = vec![MailboxId::from("archive")];
    let gateway = MutationGateway::with_sync_batch(1, observe_batch(record));

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync runs");

    assert!(
        service
            .list_pending_operations(&account_id)
            .expect("pending list")
            .is_empty(),
        "sync's post-flush settles and removes the pending assertion",
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account_id, &MessageId::from("message-1"))
            .expect("projection lookup"),
        vec![MailboxId::from("archive")],
        "canonical converges to the archived state",
    );
}

// --- S2: optimistic write-through + settle from readback -----------------------

#[tokio::test]
async fn keyword_mutation_writes_through_to_projection() {
    // S2 write-through for keywords: a setKeywords applies to canonical at once.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

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
        .expect("keyword assertion queues");

    let adds = store
        .keyword_adds
        .lock()
        .expect("keyword adds lock poisoned");
    assert_eq!(
        adds.len(),
        1,
        "the keyword assertion writes through to canonical"
    );
    assert!(adds[0].1.iter().any(|keyword| keyword == "$flagged"));
}

#[tokio::test]
async fn settle_adopts_the_readback_over_the_optimistic_value() {
    // Settle is authoritative: when the provider's readback differs from the
    // optimistic write-through (e.g. a server-side rule moved the message),
    // canonical adopts the readback, not the local guess.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let mut readback = sample_message_record("message-1", 0, false);
    readback.mailbox_ids = vec![MailboxId::from("spam")];
    gateway
        .readbacks
        .lock()
        .expect("readbacks lock poisoned")
        .push(posthaste_domain_model::MessageReadback::Present(readback));

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
            .expect("projection lookup"),
        vec![MailboxId::from("archive")],
        "optimistic write-through before flush",
    );

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("projection lookup"),
        vec![MailboxId::from("spam")],
        "settle adopts the provider readback over the optimistic value",
    );
}

#[tokio::test]
async fn settle_folds_remaining_unsettled_ops_over_the_readback() {
    // settle-completeness: settling one op preserves the others. `project_record`
    // folds the still-unsettled assertions over the provider readback.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

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
        .expect("flag assertion queues");
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

    let pending = service
        .list_pending_operations(&account)
        .expect("pending list");
    let archive_op = pending
        .iter()
        .find(|op| op.kind == OperationKind::ReplaceMailboxes)
        .expect("archive op should be pending")
        .clone();

    // The flag's readback: the provider applied the flag but not (yet) the archive.
    let mut readback = sample_message_record("message-1", 0, false);
    readback.keywords = vec!["$flagged".to_string()];
    let projected =
        super::super::message_queries::project_record(readback, std::slice::from_ref(&archive_op))
            .expect("project_record succeeds")
            .expect("the message is still present");

    assert_eq!(
        projected.mailbox_ids,
        vec![MailboxId::from("archive")],
        "the still-unsettled archive op is preserved when the flag settles",
    );
    assert!(
        projected
            .keywords
            .iter()
            .any(|keyword| keyword == "$flagged"),
        "the settled flag is carried in the readback base",
    );
}

#[tokio::test]
async fn rejected_mutation_reverts_canonical_and_settles_failed() {
    // A provider rejection still carries a readback (the unchanged state); settle
    // writes it (reverting the optimistic change) and the settlement is Failed so
    // the failure can surface.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let unchanged = sample_message_record("message-1", 0, false); // mailbox_ids = [inbox]
    *gateway
        .reject_next
        .lock()
        .expect("reject_next lock poisoned") = Some((
        posthaste_domain_model::MessageReadback::Present(unchanged),
        "permission denied".to_string(),
    ));

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
            .expect("projection lookup"),
        vec![MailboxId::from("archive")],
        "optimistic write-through before flush",
    );

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("projection lookup"),
        vec![MailboxId::from("inbox")],
        "the rejected change is reverted from the readback",
    );
    assert!(
        service
            .list_pending_operations(&account)
            .expect("pending list")
            .is_empty(),
        "the rejected op is settled and removed",
    );
    let settled = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_OPERATION_SETTLED)
        .expect("a settlement event is emitted");
    let settlement: OperationSettlement =
        serde_json::from_value(settled.payload.clone()).expect("settlement payload");
    assert!(
        matches!(settlement.outcome, OperationOutcome::Failed),
        "the rejection settles as Failed",
    );
    assert_eq!(settlement.error.as_deref(), Some("permission denied"));
}

#[tokio::test]
async fn unsettled_message_ids_tracks_queued_then_settled_assertions() {
    // The M35 durable snapshot guard folds/protects exactly the messages this
    // set names. A queued optimistic mutation puts its message in the set;
    // settling it (flush/ack) removes it, so the sync applies plain provider
    // state to that message (no stale overlay).
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
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

    let unsettled = service
        .unsettled_message_ids(&account)
        .expect("unsettled set");
    assert!(
        unsettled.contains("message-1"),
        "a queued optimistic mutation marks its message unsettled (sync-guarded)",
    );

    // Flush settles + removes the op, so the message is no longer guarded.
    let gateway = MutationGateway::with_revision(1);
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush settles the assertion");
    assert!(
        service
            .unsettled_message_ids(&account)
            .expect("unsettled set")
            .is_empty(),
        "once settled, the message is no longer unsettled and sync applies to it",
    );
}
