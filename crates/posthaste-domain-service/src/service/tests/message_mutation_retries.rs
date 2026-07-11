use super::*;

#[tokio::test]
async fn archive_mutation_folds_into_overlay() {
    // NS1 overlay plane: a message mutation queues its op and folds base + the
    // unsettled assertions into the OVERLAY row. Base/canonical is untouched —
    // sync (and the settle readback) is its only writer; reads serve the
    // effective (overlay-first) merge.
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

    let overlay = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned");
    let row = overlay
        .get("message-1")
        .expect("the mutation folds an overlay entry for the message")
        .as_ref()
        .expect("a mailbox replace folds to a row, not a tombstone");
    assert_eq!(
        row.mailbox_ids,
        vec![MailboxId::from("archive")],
        "the overlay row holds the asserted membership",
    );
    drop(overlay);
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("base mailbox lookup"),
        vec![MailboxId::from("inbox")],
        "base/canonical is untouched by the mutation",
    );
    assert!(
        store
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned")
            .is_empty(),
        "no base write rode the mutation",
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
async fn mixed_message_mutations_fold_cumulatively_into_overlay() {
    // NS1: several unsettled assertions on one message fold in queue order
    // into a single overlay row — the flag and the move are both visible in
    // the cumulative fold, while base stays sync-owned.
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
        "provider cursor advances only by sync, not local optimistic mutations",
    );
    {
        let overlay = store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned");
        let row = overlay
            .get("message-1")
            .expect("the mutations fold an overlay entry for the message")
            .as_ref()
            .expect("state assertions fold to a row, not a tombstone");
        assert!(
            row.keywords.iter().any(|keyword| keyword == "$flagged"),
            "the earlier keyword assertion is preserved in the cumulative fold",
        );
        assert_eq!(
            row.mailbox_ids,
            vec![MailboxId::from("archive")],
            "the later mailbox assertion wins in the cumulative fold",
        );
    }
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("base mailbox lookup"),
        vec![MailboxId::from("inbox")],
        "base/canonical is untouched by the mutations",
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
    // get_conversation returns the store's conversation view directly — the
    // service performs no outbox fold of its own. Under NS1 optimism lives in
    // the overlay plane and the real conversation reader serves the effective
    // (overlay-folded) view in SQL; here (TestStore's conversation_view is a
    // fixture decoupled from the write path) we assert get_conversation
    // returns it verbatim.
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

    // A pending archive does not fold into this service read; the view
    // reflects whatever the store's view serves.
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
    // NS1: a flushed message assertion settles from the provider readback
    // (set+get) and is removed at flush. The RAW readback is the new base
    // (written via the sync write path); the overlay entry retires with the
    // settled op, so reads show provider truth.
    //
    // @spec docs/eph/RFC-L2-client-replication-model#6-the-runtime-substrate-base--overlay--effective-d167d169
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

    // Base reflects the settled readback (raw, via the sync write path) and
    // the overlay entry retired with the op.
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("base mailbox lookup"),
        vec![MailboxId::from("archive")],
        "settle wrote the raw provider readback to base",
    );
    assert!(
        store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .is_empty(),
        "no overlay entry remains once the sole op settled",
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
    // `sync_account` flushes the outbox after observing, so a pending
    // assertion settles from the readback and is removed — settlement rides the
    // flush whether triggered directly or by a sync's post-flush. (The old
    // rest-in-applied/no-premature-retire mechanism this test used is gone.)
    //
    // @spec docs/eph/RFC-L2-client-replication-model#6-the-runtime-substrate-base--overlay--effective-d167d169
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

// --- NS1: optimistic fold into the overlay + settle from readback --------------

#[tokio::test]
async fn keyword_mutation_folds_into_overlay() {
    // NS1: a setKeywords folds base + the queued assertion into the overlay
    // row at once; base/canonical is never written by the mutation.
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

    let overlay = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned");
    let row = overlay
        .get("message-1")
        .expect("the mutation folds an overlay entry for the message")
        .as_ref()
        .expect("a keyword assertion folds to a row, not a tombstone");
    assert!(
        row.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the overlay row carries the flag folded over the synthetic base",
    );
    drop(overlay);
    assert!(
        store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .is_empty(),
        "the mutation never writes base/canonical (sync is base's only writer)",
    );
}

#[tokio::test]
async fn settle_adopts_the_readback_over_the_optimistic_value() {
    // Settle is authoritative: when the provider's readback differs from the
    // optimistic fold (e.g. a server-side rule moved the message), base adopts
    // the RAW readback verbatim — no optimism is folded into base (NS1) — and
    // the overlay entry retires with its op, so reads show provider truth.
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
        .push(posthaste_domain_model::MessageReadback::Present(
            readback.clone(),
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
    {
        let overlay = store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned");
        let row = overlay
            .get("message-1")
            .expect("the mutation folds an overlay entry")
            .as_ref()
            .expect("a mailbox replace folds to a row, not a tombstone");
        assert_eq!(
            row.mailbox_ids,
            vec![MailboxId::from("archive")],
            "optimistic fold lives in the overlay before flush",
        );
    }

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    let applied = store
        .applied_messages
        .lock()
        .expect("applied messages lock poisoned");
    let settled = applied
        .iter()
        .rev()
        .find(|record| record.id == MessageId::from("message-1"))
        .expect("settle wrote the readback to base");
    assert_eq!(
        serde_json::to_value(settled).expect("record serializes"),
        serde_json::to_value(&readback).expect("record serializes"),
        "base adopts the provider readback VERBATIM (raw, not folded)",
    );
    drop(applied);
    assert!(
        !store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .contains_key("message-1"),
        "the overlay entry retired with its settled op — base shows through",
    );
}

#[tokio::test]
async fn settle_refolds_remaining_unsettled_ops_into_the_overlay() {
    // settle-completeness (NS1): settling one op preserves the others — base
    // receives the RAW readback (never a fold), and the still-unsettled
    // assertions are RE-FOLDED over that new base in the OVERLAY entry.
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

    // Park the archive op outside the flushable set (still unsettled, still
    // folded) so this flush settles ONLY the flag.
    let pending = service
        .list_pending_operations(&account)
        .expect("pending list");
    let archive_op = pending
        .iter()
        .find(|op| op.kind == OperationKind::ReplaceMailboxes)
        .expect("archive op should be pending")
        .clone();
    store
        .update_operation_state(&archive_op.id, OperationState::Applied, 1, None)
        .expect("park the archive op as applied");

    // The flag's readback: the provider applied the flag but not (yet) the archive.
    let gateway = MutationGateway::with_revision(1);
    let mut readback = sample_message_record("message-1", 0, false);
    readback.keywords = vec!["$flagged".to_string()];
    gateway
        .readbacks
        .lock()
        .expect("readbacks lock poisoned")
        .push(posthaste_domain_model::MessageReadback::Present(
            readback.clone(),
        ));

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush settles the flag");

    {
        let applied = store
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned");
        let settled = applied
            .iter()
            .rev()
            .find(|record| record.id == MessageId::from("message-1"))
            .expect("settle wrote the readback to base");
        assert_eq!(
            serde_json::to_value(settled).expect("record serializes"),
            serde_json::to_value(&readback).expect("record serializes"),
            "base received the RAW readback — the pending archive was not folded in",
        );
    }
    let overlay = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned");
    let row = overlay
        .get("message-1")
        .expect("the unsettled archive keeps an overlay entry")
        .as_ref()
        .expect("the refold is a row, not a tombstone");
    assert_eq!(
        row.mailbox_ids,
        vec![MailboxId::from("archive")],
        "the still-unsettled archive op is refolded over the new base",
    );
    assert!(
        row.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the settled flag is carried in the readback base under the refold",
    );
}

#[tokio::test]
async fn rejected_mutation_settles_failed_and_bases_the_raw_readback() {
    // A provider rejection still carries a readback (the unchanged server
    // state); settle writes it to base RAW, the overlay entry retires with the
    // settled op (the optimistic fold vanishes from reads), and the settlement
    // is Failed so the failure can surface.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let unchanged = sample_message_record("message-1", 0, false); // mailbox_ids = [inbox]
    *gateway
        .reject_next
        .lock()
        .expect("reject_next lock poisoned") = Some((
        posthaste_domain_model::MessageReadback::Present(unchanged.clone()),
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
    {
        let overlay = store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned");
        let row = overlay
            .get("message-1")
            .expect("the mutation folds an overlay entry")
            .as_ref()
            .expect("a mailbox replace folds to a row, not a tombstone");
        assert_eq!(
            row.mailbox_ids,
            vec![MailboxId::from("archive")],
            "optimistic fold lives in the overlay before flush",
        );
    }

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    {
        let applied = store
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned");
        let settled = applied
            .iter()
            .rev()
            .find(|record| record.id == MessageId::from("message-1"))
            .expect("settle wrote the rejection readback to base");
        assert_eq!(
            serde_json::to_value(settled).expect("record serializes"),
            serde_json::to_value(&unchanged).expect("record serializes"),
            "base received the RAW readback — the unchanged server state",
        );
    }
    assert!(
        !store
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .contains_key("message-1"),
        "the overlay entry is gone after settlement (no remaining ops)",
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
    // The unsettled set names exactly the messages whose overlay entries the
    // NS1 sweep re-derives. A queued optimistic mutation puts its message in
    // the set; settling it (flush/ack) removes it, so the overlay retires and
    // base (plain provider state) shows through for that message.
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

/// BE-H2: a transient-failing ("poisoned") op stops halting the drain after
/// the skip threshold — the ops behind it flush, and it stays pending
/// (retryable, cancelable) rather than wedging the account forever.
#[tokio::test]
async fn poisoned_transient_op_stops_wedging_the_outbox_after_threshold() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("state-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    // The first queued op is poisoned: its next three gateway calls fail with
    // a NETWORK (transient) error; the healthy op behind it succeeds via the
    // default revision path once the queue of scripted errors is drained.
    let gateway = MutationGateway::with_revision(1);
    for _ in 0..3 {
        gateway
            .set_keywords_results
            .lock()
            .unwrap()
            .push(Err(GatewayError::Network("poisoned".to_string())));
    }
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
        .expect("poisoned op queues");
    service
        .set_keywords(
            &account,
            &MessageId::from("message-2"),
            &SetKeywordsCommand {
                add: vec!["$seen".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("healthy op queues");

    // Two passes below the threshold: the drain stops at the poisoned head
    // (offline-safe default), so BOTH ops remain pending.
    for _ in 0..2 {
        let _ = service.flush_account(&account, &gateway).await;
    }
    let pending = service.list_pending_operations(&account).expect("pending");
    assert!(
        pending.iter().any(|op| op.entity.id == "message-2"),
        "below threshold the healthy op is still stuck behind the poison"
    );

    // Third pass crosses the threshold: the poisoned op is SKIPPED and the
    // healthy op behind it flushes and settles.
    let _ = service.flush_account(&account, &gateway).await;
    let pending = service.list_pending_operations(&account).expect("pending");
    assert!(
        pending.iter().all(|op| op.entity.id != "message-2"),
        "past threshold the healthy op behind the poison flushed: {pending:?}"
    );
    assert!(
        pending
            .iter()
            .any(|op| op.entity.id == "message-1" && op.state == OperationState::Pending),
        "the poisoned op stays pending (retryable/cancelable), not failed"
    );
}
