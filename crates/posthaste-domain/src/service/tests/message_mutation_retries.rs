use super::*;

#[tokio::test]
async fn mixed_message_mutations_reuse_advanced_cursor() {
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
            &gateway,
        )
        .await
        .expect("first mutation should succeed");
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("message-1"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
            &gateway,
        )
        .await
        .expect("second mutation should succeed");

    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-3"
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")]
    );
}

// spec: docs/L0-testing#sync-convergence-contracts
// spec: docs/L1-sync#conflict-model
#[tokio::test]
async fn state_mismatch_refreshes_remote_projection_without_retrying_original_mutation() {
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let mut remote_message = sample_message_record("message-1", 0, false);
    remote_message.mailbox_ids = vec![MailboxId::from("archive")];
    let gateway = MutationGateway::with_sync_batch(
        2,
        SyncBatch {
            mailboxes: Vec::new(),
            messages: vec![remote_message],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "message-2".to_string(),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }],
        },
    );

    let error = service
        .set_keywords(
            &account_id,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect_err("stale mutation should still report a state mismatch");

    assert_eq!(error.code(), "state_mismatch");
    assert_eq!(
        store
            .get_cursor(&account_id, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    assert_eq!(
        store
            .get_message_mailboxes(&account_id, &MessageId::from("message-1"))
            .expect("mailbox lookup should succeed"),
        vec![MailboxId::from("archive")]
    );
    assert!(store
        .keyword_adds
        .lock()
        .expect("keyword adds lock poisoned")
        .is_empty());
}

#[tokio::test]
async fn genuine_state_mismatch_is_not_retried() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
    let config = Arc::new(TestConfig::default());
    let service = MailService::new(store.clone(), config);
    let gateway = MutationGateway::with_sync_batch(
        2,
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
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "message-2".to_string(),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }],
        },
    );

    let error = service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
            &gateway,
        )
        .await
        .expect_err("mismatch should be returned to the caller");

    assert_eq!(error.code(), "state_mismatch");
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    assert!(store
        .keyword_adds
        .lock()
        .expect("keyword adds lock poisoned")
        .is_empty());
}
