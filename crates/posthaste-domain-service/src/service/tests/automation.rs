use super::*;

#[tokio::test]
async fn sync_applies_matching_automation_tag() {
    let account_id = AccountId::from("primary");
    let account = sample_source();
    let store = Arc::new(TestStore::default());
    *store.rule_page.lock().expect("rule page lock poisoned") =
        vec![sample_message_summary("message-1", Vec::new())];
    let config = Arc::new(TestConfig {
        sources: vec![account],
        app_settings: Mutex::new(AppSettings {
            default_account_id: None,
            automation_rules: vec![sample_automation_rule()],
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![MessageRecord {
                id: MessageId::from("message-1"),
                source_thread_id: ThreadId::from("thread-1"),
                subject: Some("Welcome".to_string()),
                from_name: Some("PostHaste Updates".to_string()),
                from_email: Some("hello@example.com".to_string()),
                received_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
                mailbox_ids: vec![MailboxId::from("inbox")],
                ..Default::default()
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should apply action");

    assert!(
        !store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .is_empty(),
        "the automation assertion writes through to the canonical projection (S2)",
    );
    assert_eq!(
        *gateway.revision.lock().expect("revision lock poisoned"),
        2,
        "post-sync outbox flush applied the automation assertion to the provider",
    );
}

#[tokio::test]
async fn automation_backfill_processes_one_bounded_batch() {
    let account_id = AccountId::from("primary");
    let account = sample_source();
    let store = Arc::new(TestStore::default());
    *store.rule_page.lock().expect("rule page lock poisoned") = vec![
        sample_message_summary("message-1", Vec::new()),
        sample_message_summary("message-2", Vec::new()),
    ];
    let config = Arc::new(TestConfig {
        sources: vec![account],
        app_settings: Mutex::new(AppSettings {
            default_account_id: None,
            automation_rules: vec![sample_automation_rule()],
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    let (_events, has_more) = service
        .backfill_automation_rules_batch(&account_id, &gateway, 1)
        .await
        .expect("backfill should apply one bounded batch");

    assert!(has_more);
    assert!(
        !store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .is_empty(),
        "the automation assertion writes through to the canonical projection (S2)",
    );
    assert_eq!(
        *gateway.revision.lock().expect("revision lock poisoned"),
        2,
        "backfill outbox flush applied one automation assertion to the provider",
    );
}
