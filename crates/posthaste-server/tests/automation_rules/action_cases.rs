use posthaste_domain_model::{AutomationAction, MailboxId};

use crate::builders::{from_contains, mailbox, mailbox_role_is, message, rule, source_is};
use crate::gateway::{RecordedMutation, ScriptedGateway};
use crate::harness::RuleHarness;

#[tokio::test]
async fn global_rule_applies_only_to_matching_account() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_account("secondary", "Secondary");
    harness.save_rules(vec![rule(
        "tag-posthaste-primary",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);
    let primary_gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![
            message(
                "primary-match",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
            message(
                "primary-other",
                &["inbox"],
                "Other",
                "other@example.test",
                &[],
            ),
        ],
    );
    let secondary_gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "secondary-match",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &primary_gateway).await;
    harness.sync("secondary", &secondary_gateway).await;

    assert_eq!(
        harness.message_keywords("primary", "primary-match"),
        vec!["newsletter".to_string()]
    );
    assert!(harness
        .message_keywords("primary", "primary-other")
        .is_empty());
    assert!(harness
        .message_keywords("secondary", "secondary-match")
        .is_empty());
    assert_eq!(primary_gateway.mutations().len(), 1);
    assert!(secondary_gateway.mutations().is_empty());
}

#[tokio::test]
async fn mailbox_role_condition_marks_only_matching_messages_read() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "read-inbox-posthaste",
        vec![
            source_is("primary"),
            mailbox_role_is("inbox"),
            from_contains("Posthaste"),
        ],
        vec![AutomationAction::MarkRead],
    )]);
    let gateway = ScriptedGateway::new(
        vec![
            mailbox("inbox", "Inbox", Some("inbox")),
            mailbox("archive", "Archive", Some("archive")),
        ],
        vec![
            message(
                "inbox-match",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
            message(
                "archive-match",
                &["archive"],
                "Posthaste",
                "news@posthaste.test",
                &[],
            ),
        ],
    );

    harness.sync("primary", &gateway).await;

    assert!(harness.message_is_read("primary", "inbox-match"));
    assert!(!harness.message_is_read("primary", "archive-match"));
    assert_eq!(
        gateway.mutations(),
        vec![RecordedMutation::SetKeywords {
            account_id: "primary".to_string(),
            message_id: "inbox-match".to_string(),
            add: vec!["$seen".to_string()],
            remove: Vec::new(),
        }]
    );
}

#[tokio::test]
async fn automation_actions_are_idempotent_across_repeated_syncs() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "read-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::MarkRead],
    )]);
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &gateway).await;
    harness.sync("primary", &gateway).await;

    assert!(harness.message_is_read("primary", "message-1"));
    assert_eq!(gateway.mutations().len(), 1);
}

#[tokio::test]
async fn keyword_state_actions_apply_expected_keyword_deltas() {
    let cases = [
        (
            "apply-tag",
            AutomationAction::ApplyTag {
                tag: "newsletter".to_string(),
            },
            Vec::<&str>::new(),
            vec!["newsletter".to_string()],
            vec!["newsletter".to_string()],
            Vec::new(),
        ),
        (
            "remove-tag",
            AutomationAction::RemoveTag {
                tag: "newsletter".to_string(),
            },
            vec!["newsletter"],
            Vec::new(),
            Vec::new(),
            vec!["newsletter".to_string()],
        ),
        (
            "mark-unread",
            AutomationAction::MarkUnread,
            vec!["$seen"],
            Vec::new(),
            Vec::new(),
            vec!["$seen".to_string()],
        ),
        (
            "flag",
            AutomationAction::Flag,
            Vec::<&str>::new(),
            vec!["$flagged".to_string()],
            vec!["$flagged".to_string()],
            Vec::new(),
        ),
        (
            "unflag",
            AutomationAction::Unflag,
            vec!["$flagged"],
            Vec::new(),
            Vec::new(),
            vec!["$flagged".to_string()],
        ),
    ];

    for (case, action, initial_keywords, expected_keywords, expected_add, expected_remove) in cases
    {
        let harness = RuleHarness::new();
        harness.save_account("primary", "Primary");
        harness.save_rules(vec![rule(
            case,
            vec![source_is("primary"), from_contains("Posthaste")],
            vec![action],
        )]);
        let gateway = ScriptedGateway::new(
            vec![mailbox("inbox", "Inbox", Some("inbox"))],
            vec![message(
                "message-1",
                &["inbox"],
                "Posthaste",
                "news@posthaste.test",
                &initial_keywords,
            )],
        );

        harness.sync("primary", &gateway).await;

        assert_eq!(
            harness.message_keywords("primary", "message-1"),
            expected_keywords,
            "{case} should leave the expected local keywords"
        );
        assert_eq!(
            gateway.mutations(),
            vec![RecordedMutation::SetKeywords {
                account_id: "primary".to_string(),
                message_id: "message-1".to_string(),
                add: expected_add,
                remove: expected_remove,
            }],
            "{case} should send the expected gateway keyword mutation"
        );
    }
}

#[tokio::test]
async fn move_to_mailbox_action_replaces_mailbox_membership() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    harness.save_rules(vec![rule(
        "archive-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from("archive"),
        }],
    )]);
    let gateway = ScriptedGateway::new(
        vec![
            mailbox("inbox", "Inbox", Some("inbox")),
            mailbox("archive", "Archive", Some("archive")),
        ],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "news@posthaste.test",
            &[],
        )],
    );

    harness.sync("primary", &gateway).await;

    assert_eq!(
        harness.message_mailboxes("primary", "message-1"),
        vec!["archive".to_string()]
    );
    assert_eq!(
        gateway.mutations(),
        vec![RecordedMutation::ReplaceMailboxes {
            account_id: "primary".to_string(),
            message_id: "message-1".to_string(),
            mailbox_ids: vec!["archive".to_string()],
        }]
    );
}
