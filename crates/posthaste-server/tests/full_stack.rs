//! Full-stack integration tests: real handlers + real store + real auth, driven
//! through [`posthaste_http_api_adapter::build_api_router`] via the shared [`support`]
//! harness. These close the data-level gap the security review flagged — the
//! auth-layer tests prove the allow/deny matrix, but only this proves the
//! HANDLER's SQL actually restricts an account-scoped search to that account.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens
//! @spec docs/eph/DESIGN-L1-deployment-modes

mod support;

use axum::http::StatusCode;
use support::{message, Harness};

/// Two accounts both have a conversation matching the same search term. An
/// account-scoped read token must see ONLY its own account's match — in the
/// search branch, which previously dropped the source filter.
#[tokio::test]
async fn account_scoped_conversation_search_excludes_other_accounts() {
    let harness = Harness::new();
    harness.seed_source("acct-a", "Account A");
    harness.seed_source("acct-b", "acct-a display collision");
    harness.seed_messages(
        "acct-a",
        "inbox",
        vec![message("a1", "Shared subject line", "inbox")],
    );
    harness.seed_messages(
        "acct-b",
        "inbox",
        vec![message("b1", "Shared subject line", "inbox")],
    );

    let token = harness.scoped(&["action = read", "account = acct-a"]);

    // In-scope: matching ?sourceId → 200, and the response references only
    // acct-a (the colliding acct-b match must not appear anywhere in the body).
    let (status, body) = harness
        .get_json(&token, "/v1/views/conversations?sourceId=acct-a&q=Shared")
        .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert!(
        !items.is_empty(),
        "the acct-a conversation matching the search should be returned"
    );
    let raw = body.to_string();
    assert!(raw.contains("acct-a"), "response should include acct-a");
    assert!(
        !raw.contains("acct-b"),
        "acct-b must NOT leak into an acct-a-scoped conversation search: {raw}"
    );

    // Wrong source → the account caveat is out of scope → 403.
    let (wrong, _) = harness
        .get_json(&token, "/v1/views/conversations?sourceId=acct-b&q=Shared")
        .await;
    assert_eq!(wrong, StatusCode::FORBIDDEN);

    // No source → the account caveat is unsatisfiable → 403 (the auth layer
    // forces the client to scope the request, which the handler then enforces).
    let (none, _) = harness
        .get_json(&token, "/v1/views/conversations?q=Shared")
        .await;
    assert_eq!(none, StatusCode::FORBIDDEN);
}

/// A full-scope token (no caveats) still reads the cross-account aggregate.
#[tokio::test]
async fn full_scope_conversation_search_sees_all_accounts() {
    let harness = Harness::new();
    harness.seed_source("acct-a", "Account A");
    harness.seed_source("acct-b", "acct-a display collision");
    harness.seed_messages(
        "acct-a",
        "inbox",
        vec![message("a1", "Shared subject line", "inbox")],
    );
    harness.seed_messages(
        "acct-b",
        "inbox",
        vec![message("b1", "Shared subject line", "inbox")],
    );

    let (status, body) = harness
        .get_json(&harness.full_scope(), "/v1/views/conversations?q=Shared")
        .await;
    assert_eq!(status, StatusCode::OK);
    let raw = body.to_string();
    assert!(
        raw.contains("acct-a") && raw.contains("acct-b"),
        "a full-scope token should see both accounts: {raw}"
    );
}

/// The domain-event SSE (`GET /v1/events`, restored for the posthastectl event
/// tap) opens as a live `text/event-stream` for an authorized token — proving
/// the real handler executes `runtime.subscribe_events` against the real runtime
/// (not just type-checks). An account-scoped token needs a matching `accountId`
/// filter (Filter route); a full-scope token opens unfiltered.
#[tokio::test]
async fn events_stream_opens_as_sse() {
    let harness = Harness::new();
    harness.seed_source("acct-a", "Account A");

    // Full-scope token: opens the cross-account feed.
    let (status, content_type) = harness.sse_open(&harness.full_scope(), "/v1/events").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "events stream should open for full scope"
    );
    assert_eq!(
        content_type.as_deref(),
        Some("text/event-stream"),
        "the event tap must be served as SSE"
    );

    // Account-scoped token with a matching filter also opens (Filter route).
    let scoped = harness.scoped(&["action = read", "account = acct-a"]);
    let (status, content_type) = harness
        .sse_open(&scoped, "/v1/events?accountId=acct-a")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/event-stream"));
}

/// Wire-compatibility proof for the D52 tap remount (RFC-L2-scripting S2): the
/// `/v1/events` stream now flows through `Tap<EventLogFactLog, _>`, but a resume
/// (`afterSeq`) still replays the durable backlog as the SAME NDJSON/SSE
/// `DomainEvent` frames posthastectl parses — an `id:` = seq plus a `data:`
/// carrying the event JSON — and NOT the new gap frame. The backlog frame is
/// emitted first, so the harness's first-data-frame read pins the wire shape.
#[tokio::test]
async fn events_replay_preserves_the_domain_event_wire_shape() {
    let harness = Harness::new();
    harness.save_account("acct-a", "Account A", true);
    harness.seed_source("acct-a", "Account A");
    // A mock sync appends `message.*` events to the durable `event_log`.
    harness.start_account_runtime("acct-a").await;

    // Resume from the very start: the tap replays the durable backlog before the
    // live tail, so the first SSE data frame is the oldest replayed event.
    let (status, frame) = harness
        .get_text_frame(&harness.full_scope(), "/v1/events?afterSeq=0")
        .await;
    assert_eq!(status, StatusCode::OK);

    // The unchanged DomainEvent wire shape: an `id:` (the seq) and a `data:`
    // carrying the event JSON with its `seq`/`topic`. Crucially NOT the new gap
    // frame (`event: gap` / `{"kind":"reset"}`), which a replay never emits.
    assert!(frame.contains("id:"), "a replayed event carries its seq as the SSE id: {frame}");
    assert!(frame.contains("data:"), "a replayed event carries a data payload: {frame}");
    assert!(frame.contains("\"seq\""), "the data payload is a DomainEvent: {frame}");
    assert!(frame.contains("\"topic\""), "the data payload is a DomainEvent: {frame}");
    assert!(
        !frame.contains("event: gap") && !frame.contains("\"kind\":\"reset\""),
        "a replay must not emit the gap frame: {frame}"
    );
}
