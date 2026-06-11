use super::*;

#[tokio::test]
async fn set_keywords_returns_updated_message_cursor() {
    let gateway = MockJmapGateway::default();
    let outcome = gateway
        .set_keywords(
            &AccountId::from("primary"),
            &MessageId::from("em-001"),
            Some("message-1"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("mutation should succeed");

    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Message);
    assert_eq!(cursor.state, "message-2");
}

#[tokio::test]
async fn set_mailbox_role_returns_updated_mailbox_cursor() {
    let gateway = MockJmapGateway::default();
    let outcome = gateway
        .set_mailbox_role(
            &AccountId::from("primary"),
            &MailboxId::from("mb-archive"),
            Some("mailbox-1"),
            None,
            None,
        )
        .await
        .expect("mutation should succeed");

    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Mailbox);
    assert_eq!(cursor.state, "mailbox-2");
}

#[tokio::test]
async fn set_mailbox_role_can_clear_existing_owner() {
    let gateway = MockJmapGateway::default();
    let outcome = gateway
        .set_mailbox_role(
            &AccountId::from("primary"),
            &MailboxId::from("mb-archive"),
            Some("mailbox-1"),
            Some("inbox"),
            Some(&MailboxId::from("mb-inbox")),
        )
        .await
        .expect("mutation should succeed");

    let cursor = outcome.cursor.expect("cursor should be present");
    assert_eq!(cursor.object_type, SyncObject::Mailbox);
    assert_eq!(cursor.state, "mailbox-2");
}
