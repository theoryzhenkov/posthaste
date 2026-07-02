use super::*;

#[test]
fn smart_mailbox_queries_messages_across_enabled_sources() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account_a = AccountId::from("primary");
    let account_b = AccountId::from("secondary");
    setup_source(&store, &account_a, "Primary")?;
    setup_source(&store, &account_b, "Secondary")?;

    for account in [&account_a, &account_b] {
        store.apply_sync_batch(
            account,
            &SyncBatch {
                mailboxes: vec![posthaste_domain_service::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![sample_message(
                    &format!("message-{}", account.as_str()),
                    "inbox",
                    Some("mime"),
                )],
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
    }

    let rule = SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxRole,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String("inbox".to_string()),
            })],
        },
    };

    let messages = store.query_messages_by_rule(&rule)?;

    assert_eq!(messages.len(), 2);
    assert!(messages
        .iter()
        .any(|message| message.source_id == account_a));
    assert!(messages
        .iter()
        .any(|message| message.source_id == account_b));
    Ok(())
}

#[test]
fn bulk_message_hydration_preserves_order_and_account_scoped_metadata() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account_a = AccountId::from("primary");
    let account_b = AccountId::from("secondary");
    setup_source(&store, &account_a, "Primary")?;
    setup_source(&store, &account_b, "Secondary")?;

    store.apply_sync_batch(
        &account_a,
        &SyncBatch {
            mailboxes: vec![
                posthaste_domain_service::MailboxRecord {
                    id: MailboxId::from("archive"),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain_service::MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
            ],
            messages: vec![
                MessageRecord {
                    received_at: "2026-03-31T11:00:00Z".to_string(),
                    mailbox_ids: vec![MailboxId::from("inbox")],
                    keywords: vec!["$flagged".to_string(), "zeta".to_string()],
                    ..sample_message("newer", "inbox", Some("mime-newer"))
                },
                MessageRecord {
                    received_at: "2026-03-31T10:00:00Z".to_string(),
                    mailbox_ids: vec![MailboxId::from("archive"), MailboxId::from("inbox")],
                    keywords: vec!["$seen".to_string(), "alpha".to_string()],
                    ..sample_message("shared-id", "inbox", Some("mime-a"))
                },
            ],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state-a".to_string(),
                updated_at: "2026-03-31T11:00:00Z".to_string(),
            }],
        },
    )?;

    store.apply_sync_batch(
        &account_b,
        &SyncBatch {
            mailboxes: vec![posthaste_domain_service::MailboxRecord {
                id: MailboxId::from("trash"),
                name: "Trash".to_string(),
                role: Some("trash".to_string()),
                unread_emails: 0,
                total_emails: 0,
            }],
            messages: vec![MessageRecord {
                mailbox_ids: vec![MailboxId::from("trash")],
                keywords: vec!["beta".to_string()],
                ..sample_message("shared-id", "trash", Some("mime-b"))
            }],
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "state-b".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        },
    )?;

    let listed = store.list_messages(&account_a, None)?;
    assert_eq!(
        listed
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["newer", "shared-id"]
    );
    assert_eq!(listed[0].mailbox_ids, vec![MailboxId::from("inbox")]);
    assert_eq!(
        listed[0].keywords,
        vec!["$flagged".to_string(), "zeta".to_string()]
    );
    assert_eq!(
        listed[1].mailbox_ids,
        vec![MailboxId::from("archive"), MailboxId::from("inbox")]
    );
    assert_eq!(
        listed[1].keywords,
        vec!["$seen".to_string(), "alpha".to_string()]
    );

    let queried = store.query_messages_by_rule(&SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::Keyword,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String("beta".to_string()),
            })],
        },
    })?;
    assert_eq!(queried.len(), 1);
    assert_eq!(queried[0].source_id, account_b);
    assert_eq!(queried[0].mailbox_ids, vec![MailboxId::from("trash")]);
    assert_eq!(queried[0].keywords, vec!["beta".to_string()]);
    Ok(())
}
