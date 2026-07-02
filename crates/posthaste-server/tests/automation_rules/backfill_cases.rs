use posthaste_domain_service::{AutomationAction, AutomationBackfillJobStatus};

use crate::builders::{from_contains, mailbox, message, rule, source_is};
use crate::gateway::ScriptedGateway;
use crate::harness::RuleHarness;

#[tokio::test]
async fn backfill_processes_existing_matches_in_bounded_batches() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![
            message(
                "message-1",
                &["inbox"],
                "Posthaste",
                "one@posthaste.test",
                &[],
            ),
            message(
                "message-2",
                &["inbox"],
                "Posthaste",
                "two@posthaste.test",
                &[],
            ),
        ],
    );
    harness.sync("primary", &gateway).await;
    harness.save_rules(vec![rule(
        "tag-existing-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);

    let has_more_after_first_batch = harness.backfill("primary", &gateway, 1).await;
    let has_more_after_second_batch = harness.backfill("primary", &gateway, 1).await;
    let has_more_after_third_batch = harness.backfill("primary", &gateway, 1).await;

    assert!(has_more_after_first_batch);
    assert!(has_more_after_second_batch);
    assert!(!has_more_after_third_batch);
    assert_eq!(
        vec![
            harness.message_keywords("primary", "message-1"),
            harness.message_keywords("primary", "message-2"),
        ],
        vec![
            vec!["newsletter".to_string()],
            vec!["newsletter".to_string()]
        ]
    );
    assert_eq!(gateway.mutations().len(), 2);
}

#[tokio::test]
async fn durable_backfill_job_completes_current_rules_and_reruns_changed_rules() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "one@posthaste.test",
            &[],
        )],
    );
    harness.sync("primary", &gateway).await;
    harness.save_rules(vec![rule(
        "tag-existing-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);

    let first_outcome = harness.process_backfill_job("primary", &gateway, 10).await;
    let second_outcome = harness.process_backfill_job("primary", &gateway, 10).await;

    assert_eq!(first_outcome, (true, false));
    assert_eq!(second_outcome, (false, false));
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );
    assert_eq!(
        harness.message_keywords("primary", "message-1"),
        vec!["newsletter".to_string()]
    );
    assert_eq!(gateway.mutations().len(), 1);

    harness.save_rules(vec![rule(
        "tag-existing-posthaste-again",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "followup".to_string(),
        }],
    )]);

    let changed_rules_outcome = harness.process_backfill_job("primary", &gateway, 10).await;

    assert_eq!(changed_rules_outcome, (true, false));
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );
    let mut keywords = harness.message_keywords("primary", "message-1");
    keywords.sort();
    assert_eq!(
        keywords,
        vec!["followup".to_string(), "newsletter".to_string()]
    );
    assert_eq!(gateway.mutations().len(), 2);
}

#[tokio::test]
async fn force_backfill_reset_reruns_a_completed_job() {
    let harness = RuleHarness::new();
    harness.save_account("primary", "Primary");
    let gateway = ScriptedGateway::new(
        vec![mailbox("inbox", "Inbox", Some("inbox"))],
        vec![message(
            "message-1",
            &["inbox"],
            "Posthaste",
            "one@posthaste.test",
            &[],
        )],
    );
    harness.sync("primary", &gateway).await;
    harness.save_rules(vec![rule(
        "tag-existing-posthaste",
        vec![source_is("primary"), from_contains("Posthaste")],
        vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
    )]);

    // Run the job to completion against the unchanged ruleset.
    assert_eq!(
        harness.process_backfill_job("primary", &gateway, 10).await,
        (true, false)
    );
    assert_eq!(
        harness.process_backfill_job("primary", &gateway, 10).await,
        (false, false)
    );
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );

    // Without a reset, a completed job stays completed and never re-runs even
    // though the rules are unchanged.
    assert_eq!(
        harness.process_backfill_job("primary", &gateway, 10).await,
        (false, false)
    );

    // On-demand "backfill now" forces the completed job back to pending so the
    // next batch re-applies the rules (idempotent, so keywords are unchanged).
    harness.reset_backfill();
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Pending)
    );
    assert_eq!(
        harness.process_backfill_job("primary", &gateway, 10).await,
        (true, false)
    );
    assert_eq!(
        harness.current_backfill_status("primary"),
        Some(AutomationBackfillJobStatus::Completed)
    );
    assert_eq!(
        harness.message_keywords("primary", "message-1"),
        vec!["newsletter".to_string()]
    );
}
