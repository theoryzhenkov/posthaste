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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    let events = service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("sync should apply action");

    // NS1: the automation tag rides the mutation path, so it folds into the
    // OVERLAY plane, never into base. The overlay row's folded keywords are
    // observable through the echo event the fold emitted (built from the
    // effective, overlay-first read at fold time).
    let echo = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .expect("the automation mutation emits its message.updated echo");
    assert!(
        echo.payload["keywords"]
            .as_array()
            .expect("echo carries the folded keywords")
            .iter()
            .any(|keyword| keyword == "newsletter"),
        "the overlay fold carries the automation tag (effective read, not base)",
    );
    assert!(
        store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .is_empty(),
        "the mutation never writes base/canonical (sync is base's only writer)",
    );
    assert!(
        store
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned")
            .iter()
            .all(|record| record.keywords.is_empty()),
        "base holds only raw provider truth — the tag was not folded into it",
    );
    assert_eq!(
        *gateway.revision.lock().expect("revision lock poisoned"),
        2,
        "post-sync outbox flush applied the automation assertion to the provider",
    );
    // The flush settled the op WITHOUT a readback (this gateway settles
    // blind), so base never absorbed the tag — retire-on-confirmation keeps
    // the folded overlay entry serving the tag until a later sync writes it
    // into base. No flicker back to the untagged row.
    let overlay_entry = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned")
        .get("message-1")
        .cloned()
        .flatten();
    assert!(
        overlay_entry
            .as_ref()
            .is_some_and(|row| row.keywords.iter().any(|keyword| keyword == "newsletter")),
        "a blind (no-readback) settlement keeps the folded overlay entry until base confirms",
    );
}

#[tokio::test]
async fn automation_backfill_processes_one_bounded_batch() {
    let account_id = AccountId::from("primary");
    let account = sample_source();
    // NS1: the mutation path builds its echo from the effective read, so the
    // fixture needs a base row (the synthetic mutation_state-backed base).
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    );

    let (events, has_more) = service
        .backfill_automation_rules_batch(&account_id, &gateway, 1)
        .await
        .expect("backfill should apply one bounded batch");

    assert!(has_more);
    // NS1: the bounded batch tags exactly one message through the mutation
    // path — the tag folds into the overlay (visible via the echo's folded
    // keywords), base is never written by the mutation.
    let echo = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .expect("the backfill mutation emits its message.updated echo");
    assert!(
        echo.payload["keywords"]
            .as_array()
            .expect("echo carries the folded keywords")
            .iter()
            .any(|keyword| keyword == "newsletter"),
        "the overlay fold carries the automation tag (effective read, not base)",
    );
    assert!(
        store
            .keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .is_empty(),
        "the mutation never writes base/canonical (sync is base's only writer)",
    );
    assert_eq!(
        *gateway.revision.lock().expect("revision lock poisoned"),
        2,
        "backfill outbox flush applied one automation assertion to the provider",
    );
    // flush-and-observe settled the op blind (no readback) — retire-on-
    // confirmation keeps the folded entry until a sync writes the tag into
    // base, so the tagged view never flickers back.
    let overlay_entry = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned")
        .values()
        .next()
        .cloned()
        .flatten();
    assert!(
        overlay_entry
            .as_ref()
            .is_some_and(|row| row.keywords.iter().any(|keyword| keyword == "newsletter")),
        "a blind settlement keeps the folded overlay entry until base confirms",
    );
}
