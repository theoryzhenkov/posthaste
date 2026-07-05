//! M40 / D110a — the CONFIRMED F1 hotfix, proven at the server-contract level
//! over the REAL `/v1` router.
//!
//! Field symptom: every laptop sleep >5min permanently freezes the web client's
//! live updates. The chain (RFC-L2-client-resilience §2, F1): the server reaps an
//! idle link at `SESSION_IDLE_TTL=300s` (or a daemon restart drops it) → on
//! reconnect the near-end engine's subscribe GET hits the dead link → the runtime
//! returns **404** before the SSE body → the engine used to classify 4xx Permanent
//! and `run()` returned, halting all live updates until reload.
//!
//! The engine half of the fix (re-prepare on a stale-link 404/410, stay Permanent
//! on a 401/403 auth refusal) is unit-tested in `posthaste-link-near-end`
//! (`engine::tests::stale_link_*` / `auth_refused_*`). These tests pin the SERVER
//! contract that fix depends on, and the end-to-end resume shape it enables:
//!
//!  1. a reaped/dead link's stream GET is **404** (the re-prepare signal), which
//!     is *distinct* from the **401** an auth failure yields (the Permanent
//!     signal) — grounding the engine's classification split in real responses;
//!  2. after re-preparing a FRESH link (what the engine now does instead of
//!     halting), the stream delivers live updates again — a mutation's terminal
//!     notification reaches the subscriber, and a re-opened view's base snapshot
//!     is served on the fresh link — i.e. resume WITHOUT a page reload.
//!
//! spec: docs/eph/RFC-L2-client-resilience.md (F1, D110a, M40)

mod support;

use axum::http::StatusCode;
use support::{message, Harness};

/// Stand up an account with one seeded inbox message and a live runtime.
async fn seeded_harness() -> Harness {
    let harness = Harness::new();
    harness.save_account("primary", "Primary", true);
    harness.seed_source("primary", "Primary");
    harness.seed_messages(
        "primary",
        "mb-inbox",
        vec![message("em-001", "Subject", "mb-inbox")],
    );
    harness.start_account_runtime("primary").await;
    harness
}

async fn open_link(harness: &Harness, token: &str) -> String {
    let (status, body) = harness
        .post_json(
            token,
            "/v1/runtime/sessions?sourceId=primary",
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["linkId"]
        .as_str()
        .expect("link id serializes")
        .to_string()
}

// (1) The stale-link contract: a link torn down server-side (close_link stands in
// for the idle-reap at SESSION_IDLE_TTL / a daemon restart — identical registry
// teardown) makes its stream GET return 404, and a 404 is NOT what an auth failure
// returns (401). These are exactly the two arms of the engine's D110a split:
// 404/410 → re-prepare a fresh link, 401/403 → Permanent halt.
#[tokio::test]
async fn a_reaped_link_streams_404_while_an_auth_failure_streams_401() {
    let harness = seeded_harness().await;
    let token = harness.full_scope();

    let link_id = open_link(&harness, &token).await;
    let stream_path = format!("/v1/runtime/sessions/{link_id}/stream?sourceId=primary");

    // A live link streams fine (200 + an SSE body).
    let (live_status, content_type) = harness.sse_open(&token, &stream_path).await;
    assert_eq!(live_status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/event-stream"));

    // Reap it server-side (the sleep>5min / restart shape).
    let (close_status, _) = harness
        .delete_json(
            &token,
            &format!("/v1/runtime/sessions/{link_id}?sourceId=primary"),
        )
        .await;
    assert_eq!(close_status, StatusCode::OK);

    // The dead link's stream GET is 404 — the RePrepare signal (NOT Permanent).
    let (reaped_status, _) = harness.get_json(&token, &stream_path).await;
    assert_eq!(
        reaped_status,
        StatusCode::NOT_FOUND,
        "a reaped/dead link's stream must be 404 so the engine re-prepares, not halts"
    );

    // A genuine auth refusal on the SAME route is 401 — the Permanent signal the
    // engine must keep halting on. This is why the classification splits on status.
    let (auth_status, _) = harness.get_json("not-a-valid-token", &stream_path).await;
    assert_eq!(
        auth_status,
        StatusCode::UNAUTHORIZED,
        "an auth failure is 401, distinct from a stale-link 404"
    );
}

// (2a) Resume without reload: after the old link is reaped, a FRESH link (what the
// engine's re-prepare opens) delivers live updates again — a mutation's terminal
// notification reaches the fresh link's stream subscriber.
#[tokio::test]
async fn a_fresh_link_after_a_reap_delivers_a_live_notification() {
    let harness = seeded_harness().await;
    let token = harness.full_scope();

    // The reaped old link (the sleep window).
    let old_link = open_link(&harness, &token).await;
    harness
        .delete_json(
            &token,
            &format!("/v1/runtime/sessions/{old_link}?sourceId=primary"),
        )
        .await;

    // The engine's D110a response: prepare a FRESH link and carry on.
    let fresh_link = open_link(&harness, &token).await;
    assert_ne!(fresh_link, old_link, "re-prepare yields a new link id");

    // A live change on the fresh link.
    let (mutation_status, mutation_body) = harness
        .post_json(
            &token,
            &format!("/v1/runtime/sessions/{fresh_link}/mutations?sourceId=primary"),
            serde_json::json!({
                "name": "message.setKeywords",
                "clientMutationId": "reprepare-1",
                "args": {
                    "sourceId": "primary",
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }
            }),
        )
        .await;
    assert_eq!(mutation_status, StatusCode::OK, "{mutation_body}");

    // The terminal notification reaches the fresh link's subscriber — live updates
    // resumed end-to-end, no page reload.
    let (stream_status, frame) = harness
        .get_text_frame(
            &token,
            &format!("/v1/runtime/sessions/{fresh_link}/stream?afterSeq=0&sourceId=primary"),
        )
        .await;
    assert_eq!(stream_status, StatusCode::OK);
    assert!(
        frame.contains(r#""type":"mutationNotification""#),
        "{frame}"
    );
    assert!(
        frame.contains(r#""notification":{"type":"confirmed"}"#),
        "{frame}"
    );
    assert!(
        frame.contains(r#""clientMutationId":"reprepare-1""#),
        "{frame}"
    );
}

// (2b) The part-(3) finding, made concrete: for a *view* to update after
// re-prepare, the CLIENT must re-open (re-register) it on the fresh link — the
// fresh link starts with zero open views server-side. When the client does so,
// the fresh link serves the view's base snapshot. (Wiring this re-open into the
// client's recovery edge is D110b/D112 = M42/M44; M40 only makes the stream
// resume so that re-open has a live link to target.)
#[tokio::test]
async fn a_re_opened_view_on_the_fresh_link_serves_its_base_snapshot() {
    let harness = seeded_harness().await;
    let token = harness.full_scope();

    let old_link = open_link(&harness, &token).await;
    harness
        .delete_json(
            &token,
            &format!("/v1/runtime/sessions/{old_link}?sourceId=primary"),
        )
        .await;

    let fresh_link = open_link(&harness, &token).await;

    // The client re-registers its view on the fresh link (the required recovery
    // action — nothing does this automatically today; see the M40 report).
    let (view_status, view_body) = harness
        .post_json(
            &token,
            &format!("/v1/runtime/sessions/{fresh_link}/views?sourceId=primary"),
            serde_json::json!({
                "descriptor": {
                    "family": "mailList",
                    "payload": {
                        "query": "in:primary/mb-inbox",
                        "presentation": {
                            "kind": "messages",
                            "limit": 10,
                            "cursor": null,
                            "sortField": "date",
                            "sortDirection": "desc"
                        },
                        "visibility": null
                    }
                }
            }),
        )
        .await;
    assert_eq!(view_status, StatusCode::OK, "{view_body}");

    // The fresh link serves the re-opened view's base snapshot — views update
    // again once re-registered.
    let (stream_status, frame) = harness
        .get_text_frame(
            &token,
            &format!("/v1/runtime/sessions/{fresh_link}/stream?afterSeq=0&sourceId=primary"),
        )
        .await;
    assert_eq!(stream_status, StatusCode::OK);
    assert!(frame.contains(r#""type":"viewSnapshot""#), "{frame}");
    assert!(frame.contains(r#""id":"em-001""#), "{frame}");
}
