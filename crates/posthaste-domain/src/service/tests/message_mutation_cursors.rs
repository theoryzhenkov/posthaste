use super::*;

#[tokio::test]
async fn consecutive_keyword_mutations_keep_local_cursor_and_queue_base_cursor() {
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
        .expect("flagging should apply locally");
    service
        .set_keywords(
            &account,
            &MessageId::from("message-1"),
            &SetKeywordsCommand {
                add: Vec::new(),
                remove: vec!["$flagged".to_string()],
            },
        )
        .await
        .expect("unflagging should apply locally");

    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)
            .expect("cursor lookup should succeed")
            .expect("cursor should exist")
            .state,
        "message-1",
        "local-first apply must not advance the provider sync cursor",
    );
    let pending = service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    assert_eq!(pending.len(), 2);
    assert!(pending
        .iter()
        .all(|op| op.kind == OperationKind::SetKeywords));
    assert!(pending
        .iter()
        .all(|op| op.base_cursor.as_deref() == Some("message-1")));
    assert_eq!(pending[1].depends_on.as_ref(), Some(&pending[0].id));
}
