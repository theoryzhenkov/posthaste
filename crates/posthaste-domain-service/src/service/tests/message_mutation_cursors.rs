use super::*;

#[tokio::test]
async fn consecutive_keyword_assertions_keep_local_cursor_and_do_not_chain() {
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
    // The two keyword assertions coalesce into a single merged op.
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, OperationKind::SetKeywords);
    let command =
        serde_json::from_value::<SetKeywordsCommand>(pending[0].payload.clone()).expect("payload");
    assert!(command.add.is_empty(), "flag then unflag nets no additions");
    assert_eq!(command.remove, vec!["$flagged".to_string()]);
}
