use axum::http::StatusCode;
use posthaste_server::token::mint_with_caveats;

use crate::support::{full_scope, status, test_root_key};

#[tokio::test]
async fn mint_route_allows_full_scope_token() {
    let t = full_scope();
    assert_eq!(status(&t, "POST", "/v1/auth/tokens").await, StatusCode::OK);
}

#[tokio::test]
async fn mint_route_allows_unscoped_manage_token() {
    // A token carrying only `action = manage` (no resource caveat) may mint.
    let t = mint_with_caveats(&test_root_key(), &["action = manage"]);
    assert_eq!(status(&t, "POST", "/v1/auth/tokens").await, StatusCode::OK);
}

#[tokio::test]
async fn mint_route_rejects_non_manage_token() {
    // A read-only token cannot mint — the route requires the `manage` action.
    let t = mint_with_caveats(&test_root_key(), &["action = read"]);
    assert_eq!(
        status(&t, "POST", "/v1/auth/tokens").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn mint_route_rejects_resource_scoped_token() {
    // An account-scoped token carries an `account` caveat, which is unsatisfiable
    // on this resource-less route (ResourceShape::empty) → 403. This forces
    // minting through a full-scope / unscoped-manage caller, so the handler's
    // attenuation always starts from a token that is at least as broad.
    let t = mint_with_caveats(&test_root_key(), &["action = manage", "account = acct-a"]);
    assert_eq!(
        status(&t, "POST", "/v1/auth/tokens").await,
        StatusCode::FORBIDDEN
    );
}
