use super::*;

#[test]
fn mailbox_roles_preserve_serialized_strings() {
    let values = MailboxRole::ALL
        .into_iter()
        .map(MailboxRole::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        ["inbox", "archive", "drafts", "sent", "junk", "trash"]
    );
    assert_eq!(MailboxRole::parse("sent"), Some(MailboxRole::Sent));
    assert_eq!(MailboxRole::parse("Sent"), None);
    assert_eq!(MailboxRole::parse("all"), None);
    assert_eq!(
        serde_json::to_string(&MailboxRole::Inbox).expect("serialize role"),
        "\"inbox\""
    );
    assert_eq!(
        serde_json::from_str::<MailboxRole>("\"trash\"").expect("deserialize role"),
        MailboxRole::Trash
    );
}

#[test]
fn system_keywords_preserve_serialized_strings() {
    let values = SystemKeyword::ALL
        .into_iter()
        .map(SystemKeyword::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        ["$seen", "$draft", "$flagged", "$answered", "$forwarded"]
    );
    assert_eq!(
        SystemKeyword::parse("$flagged"),
        Some(SystemKeyword::Flagged)
    );
    assert_eq!(SystemKeyword::parse("flagged"), None);
    assert_eq!(
        serde_json::to_string(&SystemKeyword::Seen).expect("serialize keyword"),
        "\"$seen\""
    );
    assert_eq!(
        serde_json::from_str::<SystemKeyword>("\"$draft\"").expect("deserialize keyword"),
        SystemKeyword::Draft
    );
}
