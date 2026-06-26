//! Flicker diagnosis fixture: drive a real mutation through the runtime *while a
//! provider sync re-serves the view*, capture the emitted `RuntimeFrame` stream
//! (Layer A), replay it through the real `EntityStore` reconciliation — the
//! `ReplicaProbe`, the same optimism/absorption code the browser runs via WASM
//! (Layer B) — and assert the rendered rows never flicker: no observable field
//! reverts, no row disappears then reappears.
//!
//! The mock driver serves a canned dataset (`em-001`..`em-003` in `mb-inbox`)
//! and re-serves it on every sync **including a background poll** — so each case
//! exercises a mutation interleaved with a provider re-serve. Crucially the
//! re-serve carries the *canned* (pre-mutation) state, so it stress-tests
//! absorption: the optimism must hold against a base that never carries the
//! effect. All in-process: no browser, no `posthastectl`.
//!
// spec: docs/testing/L1#view-settlement-correctness

#[path = "common/mod.rs"]
mod common;

use std::time::Duration;

use posthaste_link_core::MessageAssertion;
use posthaste_runtime_contract::{
    ClientMutationId, MutationRequest, RuntimeCaller, RuntimeCore, RuntimeSessionId,
};
use posthaste_testkit::{Harness, ReplicaProbe};

/// A fixed capture window spanning the mock driver's background poll (~500ms)
/// re-serving the whole view — the "during provider sync" flicker trigger.
const CAPTURE_WINDOW: Duration = Duration::from_millis(1500);

fn mutation(
    name: &str,
    args: serde_json::Value,
    cmid: &str,
    session: RuntimeSessionId,
) -> MutationRequest {
    MutationRequest {
        session_id: Some(session),
        name: name.to_string(),
        args,
        client_mutation_id: ClientMutationId::new(cmid),
        context: None,
    }
}

/// Run `name`(`args`) on `target` with optimism `assertion`, interleaved with the
/// mock driver's background re-serve, and assert `target`'s row never flickers.
async fn assert_mutation_does_not_flicker(
    name: &str,
    target: &str,
    assertion: MessageAssertion,
    args: serde_json::Value,
) {
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_mock_account("a").await;

    let view = common::mail_list_view(&format!("in:{account}/mb-inbox"));
    let mut capture = harness.open_capture(view).await;

    let initial = serde_json::to_value(&capture.initial).expect("snapshot serializes");
    let mut probe = ReplicaProbe::open_view(capture.view_id.as_str(), "mb-inbox", &initial);

    // The client accepts the optimism the instant the user acts, before the
    // mutation runs against the runtime (mirrors entityStoreAdapter.runMutation).
    let cmid = "flick-op-1";
    probe.accept_mutation(cmid, target, assertion);

    let full_args = {
        let mut a = args;
        a["sourceId"] = serde_json::json!(account.as_str());
        a["messageId"] = serde_json::json!(target);
        a
    };
    harness
        .core()
        .run_mutation(
            RuntimeCaller::test(),
            mutation(name, full_args, cmid, capture.session_id.clone()),
        )
        .await
        .expect("mutation should run");

    let frames = capture.drain(CAPTURE_WINDOW).await;
    for frame in &frames {
        probe.apply_frame(frame);
    }

    // assert_no_flicker dumps the full trajectory on failure.
    probe.into_log().assert_no_flicker(target);
}

#[tokio::test]
async fn flagging_a_message_does_not_flicker_during_background_sync() {
    // em-003 is unflagged in the canned dataset; the re-serve keeps it unflagged.
    assert_mutation_does_not_flicker(
        "message.setKeywords",
        "em-003",
        MessageAssertion::SetKeywords {
            add: vec!["$flagged".into()],
            remove: vec![],
        },
        serde_json::json!({ "command": { "add": ["$flagged"], "remove": [] } }),
    )
    .await;
}

#[tokio::test]
async fn marking_read_does_not_flicker_during_background_sync() {
    // em-002 is UNREAD in the canned dataset, and every re-serve carries it
    // unread — the sharpest absorption test: the optimism must hold against a
    // base that never catches up.
    assert_mutation_does_not_flicker(
        "message.setKeywords",
        "em-002",
        MessageAssertion::SetKeywords {
            add: vec!["$seen".into()],
            remove: vec![],
        },
        serde_json::json!({ "command": { "add": ["$seen"], "remove": [] } }),
    )
    .await;
}
