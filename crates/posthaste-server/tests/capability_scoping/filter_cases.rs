use axum::http::StatusCode;
use posthaste_server::token::{attenuate, mint_with_caveats, RootKey};

use crate::support::{full_scope, status, test_root_key};

#[tokio::test]
async fn full_scope_token_still_reads_conversation_lists() {
    // No regression: a full-scope token (no caveats, fast path) still works on
    // the conversation lists, with or without a filter.
    let t = full_scope();
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-a").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/smart-mailboxes/sm-1/conversations").await,
        StatusCode::OK
    );
    assert_eq!(status(&t, "POST", "/v1/views").await, StatusCode::OK);
    assert_eq!(
        status(&t, "GET", "/v1/views/view-1/stream").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn read_only_token_reads_conversation_lists() {
    // An `action = read` token (no resource caveat) is also fine — the routes
    // are Read, and there is no account/mailbox caveat to be unsatisfiable.
    let t = attenuate(&full_scope(), "action = read").unwrap();
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/smart-mailboxes/sm-1/conversations").await,
        StatusCode::OK
    );
    assert_eq!(status(&t, "POST", "/v1/views").await, StatusCode::OK);
    assert_eq!(
        status(&t, "GET", "/v1/views/view-1/stream").await,
        StatusCode::OK
    );
}

// -- Low finding: duplicate query key fails closed. A Filter route that still
//    declares a query axis (/events on accountId) must DENY when the key appears
//    twice, rather than first-wins authorizing `?accountId=a&accountId=b`. --

#[tokio::test]
async fn duplicate_filter_param_is_denied() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    // First-wins would have matched `acct-a` and allowed; we fail closed → 403.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-a&accountId=acct-b").await,
        StatusCode::FORBIDDEN,
        "a duplicated filter key must fail closed (deny), not take the first value"
    );
    // Order-independent: duplicate is denied even if the matching value is last.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-b&accountId=acct-a").await,
        StatusCode::FORBIDDEN
    );
}

// -- Global route + scoped token. --

#[tokio::test]
async fn scoped_token_on_global_route_is_forbidden() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    assert_eq!(status(&t, "POST", "/v1/read").await, StatusCode::FORBIDDEN);
}

// -- 401 vs 403 split: a forged token is 401, not 403. --

#[tokio::test]
async fn forged_macaroon_is_unauthorized_not_forbidden() {
    // A well-formed macaroon under a DIFFERENT root key fails authenticity → 401.
    let foreign = mint_with_caveats(&RootKey::from_test_bytes([1u8; 32]), &["action = read"]);
    assert_eq!(
        status(&foreign, "GET", "/v1/accounts").await,
        StatusCode::UNAUTHORIZED
    );
    // Garbage is also 401.
    assert_eq!(
        status("not-a-macaroon", "GET", "/v1/accounts").await,
        StatusCode::UNAUTHORIZED
    );
}

// -- combined caveats AND together. --

#[tokio::test]
async fn combined_account_and_action_caveats_and_together() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a", "action = read"]);
    // In-scope account + read → allowed.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    // Right account, wrong action (send) → denied.
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::FORBIDDEN
    );
    // Right action (read), wrong account → denied.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-b/messages/m1").await,
        StatusCode::FORBIDDEN
    );
}

// -- Conversation Filter routes: account-scoped via the `sourceId` query axis. --

#[tokio::test]
async fn conversation_views_filter_on_source_id() {
    // Account-scoped read token. The route is a Filter on `sourceId`, so the
    // token is allowed only with a matching `?sourceId`; the handler then
    // result-side scopes the search to that account.
    let t = mint_with_caveats(&test_root_key(), &["action = read", "account = acct-a"]);
    // Matching source → allowed (both the plain list and the search branch).
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-a").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-a&q=hello").await,
        StatusCode::OK
    );
    // Wrong source → denied.
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?sourceId=acct-b").await,
        StatusCode::FORBIDDEN
    );
    // Absent source → the account caveat is unsatisfiable → denied (this is what
    // forces the client to scope the request, which the handler then enforces).
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/conversations?q=hello").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn runtime_session_routes_filter_on_source_id() {
    let t = mint_with_caveats(&test_root_key(), &["action = read", "account = acct-a"]);
    for (method, path) in [
        ("POST", "/v1/runtime/sessions?sourceId=acct-a"),
        ("DELETE", "/v1/runtime/sessions/session-1?sourceId=acct-a"),
        (
            "GET",
            "/v1/runtime/sessions/session-1/stream?sourceId=acct-a",
        ),
        (
            "POST",
            "/v1/runtime/sessions/session-1/views?sourceId=acct-a",
        ),
        (
            "DELETE",
            "/v1/runtime/sessions/session-1/views/view-1?sourceId=acct-a",
        ),
    ] {
        assert_eq!(
            status(&t, method, path).await,
            StatusCode::OK,
            "{method} {path}"
        );
    }
    for (method, path) in [
        ("POST", "/v1/runtime/sessions?sourceId=acct-b"),
        ("DELETE", "/v1/runtime/sessions/session-1?sourceId=acct-b"),
        (
            "GET",
            "/v1/runtime/sessions/session-1/stream?sourceId=acct-b",
        ),
        (
            "POST",
            "/v1/runtime/sessions/session-1/views?sourceId=acct-b",
        ),
        (
            "DELETE",
            "/v1/runtime/sessions/session-1/views/view-1?sourceId=acct-b",
        ),
        ("POST", "/v1/runtime/sessions"),
        ("DELETE", "/v1/runtime/sessions/session-1"),
        ("GET", "/v1/runtime/sessions/session-1/stream"),
        ("POST", "/v1/runtime/sessions/session-1/views"),
        ("DELETE", "/v1/runtime/sessions/session-1/views/view-1"),
    ] {
        assert_eq!(
            status(&t, method, path).await,
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn runtime_session_mutation_route_filters_on_source_id_and_tag_action() {
    let t = mint_with_caveats(&test_root_key(), &["action = tag", "account = acct-a"]);
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/runtime/sessions/session-1/mutations?sourceId=acct-a",
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/runtime/sessions/session-1/mutations?sourceId=acct-b",
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&t, "POST", "/v1/runtime/sessions/session-1/mutations").await,
        StatusCode::FORBIDDEN
    );

    let read_only = mint_with_caveats(&test_root_key(), &["action = read", "account = acct-a"]);
    assert_eq!(
        status(
            &read_only,
            "POST",
            "/v1/runtime/sessions/session-1/mutations?sourceId=acct-a",
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn runtime_view_routes_filter_on_source_id() {
    let t = mint_with_caveats(&test_root_key(), &["action = read", "account = acct-a"]);
    assert_eq!(
        status(&t, "POST", "/v1/views?sourceId=acct-a").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/view-1/stream?sourceId=acct-a").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "POST", "/v1/views?sourceId=acct-b").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&t, "GET", "/v1/views/view-1/stream?sourceId=acct-b").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(status(&t, "POST", "/v1/views").await, StatusCode::FORBIDDEN);
    assert_eq!(
        status(&t, "GET", "/v1/views/view-1/stream").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn smart_mailbox_conversations_filter_on_source_id() {
    let t = mint_with_caveats(&test_root_key(), &["action = read", "account = acct-a"]);
    assert_eq!(
        status(
            &t,
            "GET",
            "/v1/smart-mailboxes/sm1/conversations?sourceId=acct-a"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "GET",
            "/v1/smart-mailboxes/sm1/conversations?sourceId=acct-b"
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&t, "GET", "/v1/smart-mailboxes/sm1/conversations").await,
        StatusCode::FORBIDDEN
    );
}

// -- Token mint route (POST /auth/tokens): Manage action, no resource axis. --
