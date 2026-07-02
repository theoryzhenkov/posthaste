//! Phase 2 Slice 3: a forward action whose `context` carries a `revStep`
//! appends a reversible-op step to `rev_log` on confirmation, + a second action
//! with the same `stepId` is idempotent (re-delivery is a no-op).
//!
// spec: docs/eph/DESIGN-L2-undo-redo-revlog-contract

use posthaste_domain_service::RevCursor;
use posthaste_contract_core::{
    ClientMutationId, MutationNotification, MutationRequest, ViewDescriptor,
};
use posthaste_testkit::Harness;
use serde_json::json;

fn rev_log_view(account_id: &str) -> ViewDescriptor {
    ViewDescriptor {
        family: "revLog".to_string(),
        payload: json!({ "accountId": account_id }),
        client_self_maintained: false,
    }
}

fn rev_cursor_mutation(
    account: &str,
    cmid: &str,
    cursor_step_id: Option<&str>,
    redo_tail: &[&str],
) -> MutationRequest {
    MutationRequest {
        session_id: None, // settle() assigns the opened session
        name: "revCursor".to_string(),
        args: json!({
            "accountId": account,
            "cursorStepId": cursor_step_id,
            "redoTail": redo_tail,
        }),
        client_mutation_id: ClientMutationId::new(cmid),
        context: None,
    }
}

fn set_keywords_with_rev_step(
    account: &str,
    message_id: &str,
    cmid: &str,
    step_id: &str,
) -> MutationRequest {
    MutationRequest {
        session_id: None, // settle() assigns the opened session
        name: "message.setKeywords".to_string(),
        args: json!({
            "sourceId": account,
            "messageId": message_id,
            "command": {"add": ["$flagged"], "remove": []}
        }),
        client_mutation_id: ClientMutationId::new(cmid),
        context: Some(json!({
            "revStep": {
                "stepId": step_id,
                "diff": {"keywords": {"added": ["$flagged"], "removed": []}}
            }
        })),
    }
}

#[tokio::test]
async fn forward_action_with_rev_step_appends_to_rev_log() {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;
    harness.seed_messages(&account, &[("m-1", "inbox")]);

    let settlement = harness
        .settle(
            set_keywords_with_rev_step(account.as_str(), "m-1", "c-1", "step-1"),
            rev_log_view(account.as_str()),
        )
        .await;
    settlement.assert_confirmed();

    // The rev_log step was appended on confirmation (server-side) + the
    // cursor advanced to it: a forward action makes the new step the topmost
    // APPLIED step + truncates the redo tail. The server-authoritative cursor
    // now reflects forward actions (Slice 5b-1), so the client mirror can adopt
    // it without a client-side revCursor per forward action.
    //
    // @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    let snapshot = harness
        .store()
        .rev_log_snapshot(&account)
        .expect("rev_log snapshot reads");
    assert_eq!(snapshot.steps.len(), 1, "one step appended");
    assert_eq!(snapshot.steps[0].step_id, "step-1");
    assert_eq!(snapshot.steps[0].message_id, "m-1");
    assert_eq!(snapshot.steps[0].source_id, account.as_str());
    assert_eq!(
        snapshot.cursor,
        RevCursor {
            cursor_step_id: Some("step-1".to_string()),
            redo_tail: vec![],
        },
        "the append advances the cursor to the new step + clears the redo tail"
    );
}

#[tokio::test]
async fn forward_action_without_rev_step_does_not_append() {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;
    harness.seed_messages(&account, &[("m-1", "inbox")]);

    // A forward action with no `revStep` in its context appends nothing — the
    // rev_log append is opt-in (the client supplies the step on actions it
    // wants to be undoable). Idempotency on `step_id` is covered by the store
    // unit tests.
    let mutation = MutationRequest {
        session_id: None,
        name: "message.setKeywords".to_string(),
        args: json!({
            "sourceId": account.as_str(),
            "messageId": "m-1",
            "command": {"add": ["$flagged"], "remove": []}
        }),
        client_mutation_id: ClientMutationId::new("c-1"),
        context: None,
    };
    harness
        .settle(mutation, rev_log_view(account.as_str()))
        .await
        .assert_confirmed();

    let snapshot = harness
        .store()
        .rev_log_snapshot(&account)
        .expect("rev_log snapshot reads");
    assert!(
        snapshot.steps.is_empty(),
        "no step appended without a revStep in the context"
    );
}

#[tokio::test]
async fn rev_cursor_assigns_the_cursor() {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;
    harness.seed_messages(&account, &[("m-1", "inbox")]);

    // Forward action appends step-1 + advances the cursor to it
    // ({Some(step-1), []}) — Slice 5b-1: the server-authoritative cursor
    // reflects forward actions.
    harness
        .settle(
            set_keywords_with_rev_step(account.as_str(), "m-1", "c-1", "step-1"),
            rev_log_view(account.as_str()),
        )
        .await
        .assert_confirmed();

    // Undo: cursor → null (all undone), step-1 pushed to the redo tail. The
    // server validates step-1 exists + applies the idempotent assignment.
    harness
        .settle(
            rev_cursor_mutation(account.as_str(), "c-2", None, &["step-1"]),
            rev_log_view(account.as_str()),
        )
        .await
        .assert_confirmed();

    let snapshot = harness
        .store()
        .rev_log_snapshot(&account)
        .expect("rev_log snapshot reads");
    assert_eq!(snapshot.steps.len(), 1, "the step is still there");
    assert_eq!(snapshot.cursor.cursor_step_id, None, "cursor is all-undone");
    assert_eq!(snapshot.cursor.redo_tail, vec!["step-1".to_string()]);
}

#[tokio::test]
async fn rev_cursor_rejects_an_unknown_step() {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;
    harness.seed_messages(&account, &[("m-1", "inbox")]);

    // No forward action — the rev_log is empty. A cursor referencing a
    // non-existent step is rejected (server arbitration).
    let settlement = harness
        .settle(
            rev_cursor_mutation(account.as_str(), "c-1", Some("no-such-step"), &[]),
            rev_log_view(account.as_str()),
        )
        .await;
    assert!(
        matches!(
            settlement.settlement(),
            Some(MutationNotification::Rejected { .. })
        ),
        "revCursor with an unknown step must be rejected"
    );
}
