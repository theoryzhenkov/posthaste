use posthaste_query_grammar::parse_query;

use super::*;

#[test]
fn message_page_sorts_and_paginates() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                id: MessageId::from("message-c"),
                subject: Some("Charlie".to_string()),
                received_at: "2026-04-03T10:00:00Z".to_string(),
                ..sample_message("message-c", "inbox", Some("mime-c"))
            },
            MessageRecord {
                id: MessageId::from("message-a"),
                subject: Some("Alpha".to_string()),
                received_at: "2026-04-01T10:00:00Z".to_string(),
                ..sample_message("message-a", "inbox", Some("mime-a"))
            },
            MessageRecord {
                id: MessageId::from("message-b"),
                subject: Some("Bravo".to_string()),
                received_at: "2026-04-02T10:00:00Z".to_string(),
                ..sample_message("message-b", "inbox", Some("mime-b"))
            },
        ],
        "state",
    )?;

    let first_page = store.list_message_page(
        &account,
        None,
        2,
        None,
        MessageSortField::Subject,
        SortDirection::Asc,
    )?;
    assert_eq!(
        first_page
            .items
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["message-a", "message-b"]
    );
    let cursor = first_page
        .next_cursor
        .as_ref()
        .expect("first page should expose a next cursor");

    let second_page = store.list_message_page(
        &account,
        None,
        2,
        Some(cursor),
        MessageSortField::Subject,
        SortDirection::Asc,
    )?;
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["message-c"]
    );
    assert!(second_page.next_cursor.is_none());
    Ok(())
}

#[test]
fn message_page_paginates_empty_sort_values() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                id: MessageId::from("blank-subject"),
                subject: None,
                ..sample_message("blank-subject", "inbox", Some("mime-blank"))
            },
            MessageRecord {
                id: MessageId::from("alpha-subject"),
                subject: Some("Alpha".to_string()),
                ..sample_message("alpha-subject", "inbox", Some("mime-alpha"))
            },
        ],
        "state",
    )?;

    let first_page = store.list_message_page(
        &account,
        None,
        1,
        None,
        MessageSortField::Subject,
        SortDirection::Asc,
    )?;
    assert_eq!(first_page.items[0].id.as_str(), "blank-subject");
    assert_eq!(
        first_page
            .next_cursor
            .as_ref()
            .expect("first page should expose a next cursor")
            .sort_value,
        ""
    );

    let second_page = store.list_message_page(
        &account,
        None,
        1,
        first_page.next_cursor.as_ref(),
        MessageSortField::Subject,
        SortDirection::Asc,
    )?;
    assert_eq!(second_page.items[0].id.as_str(), "alpha-subject");
    assert!(second_page.next_cursor.is_none());
    Ok(())
}

#[test]
fn message_page_rule_query_filters_source_mailbox_and_text() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");
    setup_source(&store, &primary, "Primary")?;
    setup_source(&store, &secondary, "Secondary")?;
    seed_messages(
        &store,
        &primary,
        vec![
            MessageRecord {
                id: MessageId::from("match"),
                subject: Some("Posthaste account created".to_string()),
                mailbox_ids: vec![MailboxId::from("inbox")],
                ..sample_message("match", "inbox", Some("mime-match"))
            },
            MessageRecord {
                id: MessageId::from("wrong-mailbox"),
                subject: Some("Posthaste account created".to_string()),
                mailbox_ids: vec![MailboxId::from("archive")],
                ..sample_message("wrong-mailbox", "archive", Some("mime-archive"))
            },
        ],
        "primary-state",
    )?;
    seed_messages(
        &store,
        &secondary,
        vec![MessageRecord {
            id: MessageId::from("wrong-source"),
            subject: Some("Posthaste account created".to_string()),
            mailbox_ids: vec![MailboxId::from("inbox")],
            ..sample_message("wrong-source", "inbox", Some("mime-source"))
        }],
        "secondary-state",
    )?;

    let page = store.query_message_page_by_rule(
        &all_rule(vec![
            rule_condition(
                MailQueryField::SourceId,
                MailQueryOperator::Equals,
                "primary",
            ),
            rule_condition(
                MailQueryField::MailboxId,
                MailQueryOperator::Equals,
                "inbox",
            ),
            rule_condition(
                MailQueryField::Subject,
                MailQueryOperator::Contains,
                "Posthaste",
            ),
        ]),
        10,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.as_str(), "match");
    assert_eq!(page.items[0].source_id, primary);
    assert_eq!(page.items[0].mailbox_ids, vec![MailboxId::from("inbox")]);
    assert!(page.next_cursor.is_none());
    Ok(())
}

#[test]
fn parsed_message_query_executes_richer_filters() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary Account")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                id: MessageId::from("match"),
                source_thread_id: ThreadId::from("thread-match"),
                subject: Some("Posthaste account created".to_string()),
                mailbox_ids: vec![MailboxId::from("archive")],
                keywords: Vec::new(),
                ..sample_message("match", "archive", Some("mime-match"))
            },
            MessageRecord {
                id: MessageId::from("read-message"),
                source_thread_id: ThreadId::from("thread-match"),
                subject: Some("Posthaste account created".to_string()),
                mailbox_ids: vec![MailboxId::from("archive")],
                keywords: vec!["$seen".to_string()],
                ..sample_message("read-message", "archive", Some("mime-read"))
            },
        ],
        "state",
    )?;

    let rule = parse_query(
            "source: Primary Account in:Archive is:unread subject:account created id:match thread:thread-match",
        )
        .map_err(StoreError::Failure)?;
    let page = store.query_message_page_by_rule(
        &rule,
        10,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.as_str(), "match");
    assert!(!page.items[0].is_read);
    Ok(())
}

#[test]
fn command_writes_are_reflected_in_indexed_reads_and_counts() -> Result<(), StoreError> {
    // S2/S4: a local command write updates canonical, so the indexed reads and
    // the trigger-maintained mailbox counts reflect the optimistic state
    // directly — no read-time overlay fold. This is the property the runtime
    // recompute depends on (write-through -> indexed read).
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
        "state",
    )?;

    // A keyword write is reflected in the indexed read.
    store.set_keywords(
        &account,
        &MessageId::from("message-1"),
        None,
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        },
    )?;
    let inbox = store.list_messages(&account, Some(&MailboxId::from("inbox")))?;
    assert_eq!(inbox.len(), 1);
    assert!(
        inbox[0].is_flagged,
        "the keyword write is reflected in the indexed read",
    );

    // A mailbox move is reflected in the indexed reads AND the trigger-maintained
    // mailbox counts.
    store.replace_mailboxes(
        &account,
        &MessageId::from("message-1"),
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;
    assert!(
        store
            .list_messages(&account, Some(&MailboxId::from("inbox")))?
            .is_empty(),
        "the move is reflected: gone from inbox",
    );
    assert_eq!(
        store
            .list_messages(&account, Some(&MailboxId::from("archive")))?
            .len(),
        1,
        "the move is reflected: present in archive",
    );

    let mailboxes = store.list_mailboxes(&account)?;
    let total = |id: &str| {
        mailboxes
            .iter()
            .find(|mailbox| mailbox.id == MailboxId::from(id))
            .map_or(-1, |mailbox| mailbox.total_emails)
    };
    assert_eq!(
        total("inbox"),
        0,
        "inbox count decremented by the move (trigger)"
    );
    assert_eq!(
        total("archive"),
        1,
        "archive count incremented by the move (trigger)"
    );

    Ok(())
}
