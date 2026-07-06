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

    // set+get: the outcome carries the message's authoritative record after the
    // change (drives optimistic settlement).
    let MessageReadback::Present(message) = outcome.message.expect("set+get returns the message")
    else {
        panic!("keyword change leaves the message present");
    };
    assert_eq!(message.id, MessageId::from("em-001"));
    assert!(message.keywords.iter().any(|keyword| keyword == "$flagged"));
}

#[tokio::test]
async fn replace_mailboxes_returns_updated_message_record() {
    let gateway = MockJmapGateway::default();
    let outcome = gateway
        .replace_mailboxes(
            &AccountId::from("primary"),
            &MessageId::from("em-001"),
            Some("message-1"),
            &[MailboxId::from("mb-archive")],
        )
        .await
        .expect("mutation should succeed");

    let MessageReadback::Present(message) = outcome.message.expect("set+get returns the message")
    else {
        panic!("a mailbox replace leaves the message present");
    };
    assert_eq!(message.mailbox_ids, vec![MailboxId::from("mb-archive")]);
}

#[tokio::test]
async fn destroy_message_returns_no_record() {
    let gateway = MockJmapGateway::default();
    let outcome = gateway
        .destroy_message(
            &AccountId::from("primary"),
            &MessageId::from("em-001"),
            Some("message-1"),
        )
        .await
        .expect("mutation should succeed");

    // The message is destroyed — self-describing as Removed, not an overloaded None.
    assert!(matches!(outcome.message, Some(MessageReadback::Removed)));
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
async fn create_mailbox_returns_id_and_resync_surfaces_it() {
    let gateway = MockJmapGateway::default();
    let account = AccountId::from("primary");

    let mailbox_id = gateway
        .create_mailbox(&account, "Receipts")
        .await
        .expect("create should succeed");
    assert_eq!(mailbox_id, MailboxId::from("mb-Receipts"));

    // The resync readback (what the service performs after a create) surfaces
    // the new mailbox.
    let batch = gateway
        .sync(&account, &[], None)
        .await
        .expect("sync should succeed");
    assert!(
        batch
            .mailboxes
            .iter()
            .any(|mailbox| mailbox.id == mailbox_id && mailbox.name == "Receipts"),
        "the created mailbox appears in the resync batch"
    );
}

#[tokio::test]
async fn destroy_empty_mailbox_disappears_from_the_resync() {
    // mb-trash holds no sample messages, so destroying it needs no confirmation.
    let gateway = MockJmapGateway::default();
    let account = AccountId::from("primary");

    gateway
        .destroy_mailbox(&account, &MailboxId::from("mb-trash"), false)
        .await
        .expect("an empty mailbox destroys without confirmation");

    let batch = gateway
        .sync(&account, &[], None)
        .await
        .expect("sync should succeed");
    assert!(
        !batch
            .mailboxes
            .iter()
            .any(|mailbox| mailbox.id == MailboxId::from("mb-trash")),
        "the destroyed mailbox is gone from the resync readback",
    );
}

#[tokio::test]
async fn destroy_non_empty_mailbox_without_remove_emails_is_refused() {
    // mb-archive holds em-002 — the onDestroyRemoveEmails=false backstop refuses.
    let gateway = MockJmapGateway::default();
    let account = AccountId::from("primary");

    let error = gateway
        .destroy_mailbox(&account, &MailboxId::from("mb-archive"), false)
        .await
        .expect_err("a non-empty mailbox is refused without remove_emails");
    assert!(matches!(error, GatewayError::MailboxNotEmpty { count: 1 }));

    // The mailbox is untouched: still present in the next sync.
    let batch = gateway.sync(&account, &[], None).await.expect("sync");
    assert!(batch
        .mailboxes
        .iter()
        .any(|mailbox| mailbox.id == MailboxId::from("mb-archive")));
}

#[tokio::test]
async fn destroy_non_empty_mailbox_with_remove_emails_drops_mailbox_and_mail() {
    let gateway = MockJmapGateway::default();
    let account = AccountId::from("primary");

    gateway
        .destroy_mailbox(&account, &MailboxId::from("mb-archive"), true)
        .await
        .expect("remove_emails destroys the mailbox and its mail");

    let batch = gateway.sync(&account, &[], None).await.expect("sync");
    assert!(!batch
        .mailboxes
        .iter()
        .any(|mailbox| mailbox.id == MailboxId::from("mb-archive")));
    assert!(
        !batch
            .messages
            .iter()
            .any(|message| message.id == MessageId::from("em-002")),
        "the contained mail is removed with remove_emails=true",
    );
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
