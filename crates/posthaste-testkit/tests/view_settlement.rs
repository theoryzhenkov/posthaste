//! First view-settlement regression: a keyword toggle settles Confirmed and the
//! change reaches the client. Under option iii (single-source-view-membership)
//! an evaluable mail-list view is **self-maintained by the client** from the
//! `message.updated` firehose, so the runtime no longer re-serves it per event —
//! the assertion is that the firehose notification fired AND the runtime emitted
//! no redundant per-event view recompute (the retired #3 re-serve). No unrelated
//! view recomputes, and session seq is monotonic. (The client-side convergence
//! of the view itself is covered by the live-convergence + replica tests.)
//!
// spec: docs/testing/L1#view-settlement-correctness

#[path = "common/mod.rs"]
mod common;

use posthaste_contract_core::MutationRequest;
use posthaste_testkit::Harness;

fn set_keywords_mutation(account_id: &str, message_id: &str, cmid: &str) -> MutationRequest {
    // Built from the flat wire shape: the typed operation flattens its
    // `name`/`args` into the envelope (session_id is assigned by settle()).
    serde_json::from_value(serde_json::json!({
        "name": "message.setKeywords",
        "args": {
            "sourceId": account_id,
            "messageId": message_id,
            "command": {"add": ["$flagged"], "remove": []}
        },
        "clientMutationId": cmid,
    }))
    .expect("request builds from the flat wire shape")
}

#[tokio::test]
async fn keyword_toggle_settles_and_fires_notification_without_re_serving_the_view() {
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
    // Option iii: the mail-list view is self-maintained by the client from the
    // firehose, so the runtime fires the notification but does NOT re-serve it.
    settlement.assert_message_updated_notification();
    settlement.assert_view_not_recomputed();
    settlement.assert_only_view_recomputed();
    settlement.assert_seq_monotonic();
}
