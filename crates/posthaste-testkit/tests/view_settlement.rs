//! First view-settlement regression: a keyword toggle settles Confirmed, the
//! touched view recomputes at least once (no missed update), no unrelated view
//! recomputes (no over-broad invalidation), and session seq is monotonic.
//!
//! A `run_mutation` on a live account legitimately recomputes twice — the
//! optimistic local pass, then a sync-confirmed pass — so this asserts
//! `at_least_once`, not `exactly_once`. The settlement recorder captures both
//! phases plus the notification fan-out; `exactly_once` is reserved for
//! no-follow-up-sync scenarios.
//!
// spec: docs/testing/L1#view-settlement-correctness

#[path = "common/mod.rs"]
mod common;

use posthaste_runtime_contract::{ClientMutationId, MutationRequest};
use posthaste_testkit::Harness;

fn set_keywords_mutation(account_id: &str, message_id: &str, cmid: &str) -> MutationRequest {
    MutationRequest {
        session_id: None, // settle() assigns the opened session
        name: "message.setKeywords".to_string(),
        args: serde_json::json!({
            "sourceId": account_id,
            "messageId": message_id,
            "command": {"add": ["$flagged"], "remove": []}
        }),
        client_mutation_id: ClientMutationId::new(cmid),
        context: None,
    }
}

#[tokio::test]
async fn keyword_toggle_settles_and_recomputes_the_touched_view() {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;
    harness.seed_messages(&account, &[("m-1", "inbox"), ("m-2", "inbox")]);

    let settlement = harness
        .settle(
            set_keywords_mutation(account.as_str(), "m-1", "c-1"),
            common::mail_list_view("in:a/inbox"),
        )
        .await;

    settlement.assert_confirmed();
    settlement.assert_view_recomputed_at_least_once();
    settlement.assert_only_view_recomputed();
    settlement.assert_seq_monotonic();
}
