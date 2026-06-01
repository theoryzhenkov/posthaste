//! Full-stack integration tests: real handlers + real store + real auth, driven
//! through [`posthaste_server::build_api_router`] via the shared [`support`]
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
    harness.seed_source("acct-b", "Account B");
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
    harness.seed_source("acct-b", "Account B");
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
