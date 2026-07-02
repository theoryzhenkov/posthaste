use axum::http::StatusCode;
use posthaste_api::token::{attenuate, mint_with_caveats};

use crate::support::{full_scope, status, test_root_key};

#[tokio::test]
async fn full_scope_token_allows_all_verb_classes() {
    let t = full_scope();
    assert_eq!(status(&t, "GET", "/v1/accounts").await, StatusCode::OK);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/add-to-mailbox"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/destroy"
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "POST", "/v1/config:reload").await,
        StatusCode::OK
    );
}

// -- action caveat. --

#[tokio::test]
async fn read_only_token_allows_get_denies_writes() {
    let t = attenuate(&full_scope(), "action = read").unwrap();
    // A read passes.
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    // Send (Send action) is out of scope → 403.
    assert_eq!(
        status(&t, "POST", "/v1/sources/acct-a/commands/send").await,
        StatusCode::FORBIDDEN
    );
    // Tag and destroy are also denied.
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/destroy"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn multi_action_token_allows_each_listed_verb() {
    let t = attenuate(&full_scope(), "action = read,tag").unwrap();
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/set-keywords"
        )
        .await,
        StatusCode::OK
    );
    // move is not listed → denied.
    assert_eq!(
        status(
            &t,
            "POST",
            "/v1/sources/acct-a/commands/messages/m1/add-to-mailbox"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

// -- account caveat (Gate routes). --

#[tokio::test]
async fn account_token_gates_by_path_source() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-b/messages/m1").await,
        StatusCode::FORBIDDEN
    );
    // The account list has no scopable account axis → an account-scoped token
    // is rejected there.
    assert_eq!(
        status(&t, "GET", "/v1/accounts").await,
        StatusCode::FORBIDDEN
    );
}

// -- message caveat. --

#[tokio::test]
async fn message_token_gates_by_path_message() {
    let t = mint_with_caveats(&test_root_key(), &["message = m1"]);
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m1").await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t, "GET", "/v1/sources/acct-a/messages/m2").await,
        StatusCode::FORBIDDEN
    );
}

// -- expires caveat. --

#[tokio::test]
async fn expired_token_is_forbidden_future_is_allowed() {
    let past = attenuate(&full_scope(), "expires = 2020-01-01T00:00:00Z").unwrap();
    assert_eq!(
        status(&past, "GET", "/v1/accounts").await,
        StatusCode::FORBIDDEN
    );
    let future = attenuate(&full_scope(), "expires = 2099-01-01T00:00:00Z").unwrap();
    assert_eq!(status(&future, "GET", "/v1/accounts").await, StatusCode::OK);
}

// -- Filter route still backed by a result-side-filtered handler: GET /events
//    (keyed on accountId). A matching filter satisfies the caveat; a missing or
//    non-matching one denies. --

#[tokio::test]
async fn events_filter_route_requires_matching_account_filter() {
    let t = mint_with_caveats(&test_root_key(), &["account = acct-a"]);
    // Matching filter → allowed.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-a").await,
        StatusCode::OK
    );
    // No filter → the account axis is None → unsatisfiable → 403.
    assert_eq!(status(&t, "GET", "/v1/events").await, StatusCode::FORBIDDEN);
    // Non-matching filter → 403.
    assert_eq!(
        status(&t, "GET", "/v1/events?accountId=acct-b").await,
        StatusCode::FORBIDDEN
    );
}

// -- Conversation-list routes are now result-side scoped on `sourceId` (Tier-1):
//    an account-scoped token is allowed WITH a matching `?sourceId` (the handler
//    restricts results to that account in every branch) and denied otherwise.
//    The allow/deny matrix is covered by `conversation_views_filter_on_source_id`
//    and `smart_mailbox_conversations_filter_on_source_id` above. --
