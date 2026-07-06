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
                mailboxes: vec![posthaste_domain_model::MailboxRecord {
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
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
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
                posthaste_domain_model::MailboxRecord {
                    id: MailboxId::from("archive"),
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                },
                posthaste_domain_model::MailboxRecord {
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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
            mailboxes: vec![posthaste_domain_model::MailboxRecord {
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
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
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

/// Wraps a single leaf condition in an `All` root group — the shape the editor
/// emits for a one-condition rule.
fn single_condition_rule(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field,
                operator,
                negated: false,
                value,
            })],
        },
    }
}

#[test]
fn size_field_compiles_numeric_comparisons() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                size: 500,
                ..sample_message("small", "inbox", Some("mime-small"))
            },
            MessageRecord {
                size: 1_048_576, // exactly 1 MiB
                ..sample_message("mid", "inbox", Some("mime-mid"))
            },
            MessageRecord {
                size: 5_000_000,
                ..sample_message("large", "inbox", Some("mime-large"))
            },
        ],
        "state-size",
    )?;

    // `After` (>) 1 MiB, encoded as a byte-count string on the wire.
    let over_1mib = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::Size,
        SmartMailboxOperator::After,
        SmartMailboxValue::String("1048576".to_string()),
    ))?;
    assert_eq!(
        over_1mib
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["large"]
    );

    // `OnOrAfter` (>=) is inclusive of the exact-boundary message.
    let at_least_1mib = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::Size,
        SmartMailboxOperator::OnOrAfter,
        SmartMailboxValue::String("1048576".to_string()),
    ))?;
    let mut ids: Vec<&str> = at_least_1mib.iter().map(|m| m.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["large", "mid"]);

    // `Before` (<) compares numerically, not lexicographically: "500" < "1048576".
    let under_1mib = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::Size,
        SmartMailboxOperator::Before,
        SmartMailboxValue::String("1048576".to_string()),
    ))?;
    assert_eq!(
        under_1mib
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["small"]
    );

    // A non-numeric wire value is a type error at evaluation time.
    let err = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::Size,
        SmartMailboxOperator::Before,
        SmartMailboxValue::String("not-a-number".to_string()),
    ));
    assert!(err.is_err());
    Ok(())
}

#[test]
fn to_field_matches_recipients_in_to_json() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                to: vec![
                    Recipient {
                        name: Some("Bob Jones".to_string()),
                        email: "bob@example.com".to_string(),
                    },
                    Recipient {
                        name: None,
                        email: "carol@example.com".to_string(),
                    },
                ],
                ..sample_message("to-bob", "inbox", Some("mime-bob"))
            },
            MessageRecord {
                to: vec![Recipient {
                    name: Some("Dave".to_string()),
                    email: "dave@other.test".to_string(),
                }],
                ..sample_message("to-dave", "inbox", Some("mime-dave"))
            },
        ],
        "state-to",
    )?;

    // `Equals` matches an exact recipient email (structured per-recipient match).
    let exact = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::To,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String("carol@example.com".to_string()),
    ))?;
    assert_eq!(
        exact.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["to-bob"]
    );

    // `Contains` is case-insensitive and matches email OR display name.
    let by_domain = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::To,
        SmartMailboxOperator::Contains,
        SmartMailboxValue::String("EXAMPLE.COM".to_string()),
    ))?;
    assert_eq!(
        by_domain.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["to-bob"]
    );
    let by_name = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::To,
        SmartMailboxOperator::Contains,
        SmartMailboxValue::String("dave".to_string()),
    ))?;
    assert_eq!(
        by_name.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["to-dave"]
    );

    // `In` matches any recipient whose email is in the list.
    let in_list = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::To,
        SmartMailboxOperator::In,
        SmartMailboxValue::Strings(vec![
            "dave@other.test".to_string(),
            "nobody@nowhere.test".to_string(),
        ]),
    ))?;
    assert_eq!(
        in_list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["to-dave"]
    );

    // No recipient match returns nothing.
    let none = store.query_messages_by_rule(&single_condition_rule(
        SmartMailboxField::To,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String("ghost@example.com".to_string()),
    ))?;
    assert!(none.is_empty());
    Ok(())
}
