use super::*;

#[test]
fn list_conversations_preserves_source_names_with_commas() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary, Inc.")?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_service::MailboxRecord {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![sample_message("message-1", "inbox", Some("mime"))],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    )?;

    let page = store.list_conversations(
        Some(&account),
        None,
        10,
        None,
        ConversationSortField::default(),
        SortDirection::default(),
    )?;

    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].source_names,
        vec!["Primary, Inc.".to_string()]
    );
    assert_eq!(page.items[0].latest_source_name, "Primary, Inc.");
    Ok(())
}

#[test]
fn query_conversations_by_rule_excludes_other_accounts() -> Result<(), StoreError> {
    // The result-side scope rule (`SourceId = primary`) ANDed into a conversation
    // search MUST exclude another account's messages even when they match the rest
    // of the rule. This is the SQL-level guard behind the Tier-1 account-scoped
    // capability tokens on `/views/conversations` + smart-mailbox conversations:
    // the filter is applied at the message level before conversation grouping, so
    // a colliding subject in `secondary` must never surface.
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");
    setup_source(&store, &primary, "Primary")?;
    setup_source(&store, &secondary, "Secondary")?;
    seed_messages(
        &store,
        &primary,
        vec![MessageRecord {
            id: MessageId::from("primary-match"),
            subject: Some("Shared subject line".to_string()),
            mailbox_ids: vec![MailboxId::from("inbox")],
            ..sample_message("primary-match", "inbox", Some("mime-primary"))
        }],
        "primary-state",
    )?;
    seed_messages(
        &store,
        &secondary,
        vec![MessageRecord {
            id: MessageId::from("secondary-match"),
            subject: Some("Shared subject line".to_string()),
            mailbox_ids: vec![MailboxId::from("inbox")],
            ..sample_message("secondary-match", "inbox", Some("mime-secondary"))
        }],
        "secondary-state",
    )?;

    // A rule whose subject condition matches BOTH accounts, scoped to `primary`.
    let rule = all_rule(vec![
        rule_condition(
            SmartMailboxField::SourceId,
            SmartMailboxOperator::Equals,
            "primary",
        ),
        rule_condition(
            SmartMailboxField::Subject,
            SmartMailboxOperator::Contains,
            "Shared subject",
        ),
    ]);
    let page = store.query_conversations_by_rule(
        &rule,
        10,
        None,
        ConversationSortField::default(),
        SortDirection::default(),
    )?;

    // Only the primary account's conversation is returned, and the cross-account
    // `source_ids` aggregate never names the secondary account.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].source_ids, vec![primary]);
    assert!(
        page.items
            .iter()
            .all(|conversation| !conversation.source_ids.contains(&secondary)),
        "secondary account must not leak into a primary-scoped conversation query"
    );
    Ok(())
}
