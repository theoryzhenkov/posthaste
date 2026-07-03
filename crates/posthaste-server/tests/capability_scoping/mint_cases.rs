use axum::http::StatusCode;
use posthaste_http_api_adapter::token::mint_with_caveats;

use crate::support::{full_scope, status, test_root_key};

#[tokio::test]
async fn mint_route_allows_full_scope_token() {
    let t = full_scope();
    assert_eq!(status(&t, "POST", "/v1/auth/tokens").await, StatusCode::OK);
}

#[tokio::test]
async fn mint_route_allows_unscoped_mint_token() {
    // A token carrying only `action = mint` (no resource caveat) may mint — the
    // discovery bootstrap's shape (RFC-L2-scripting §7 ruling 11: `{mint,
    // tap:read}`) is exactly this plus `read`.
    let t = mint_with_caveats(&test_root_key(), &["action = mint"]);
    assert_eq!(status(&t, "POST", "/v1/auth/tokens").await, StatusCode::OK);
}

#[tokio::test]
async fn mint_route_rejects_unscoped_manage_token_without_mint() {
    // `manage` no longer implies `mint` (ruling 11 decouples "mint" from the
    // generic admin verb, so a least-default bootstrap can hold `mint` without
    // also holding account/settings write power). A `manage`-only token is
    // rejected — it must be granted `mint` explicitly.
    let t = mint_with_caveats(&test_root_key(), &["action = manage"]);
    assert_eq!(
        status(&t, "POST", "/v1/auth/tokens").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn mint_route_rejects_non_mint_token() {
    // A read-only token cannot mint — the route requires the `mint` action.
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
    // minting through a full-scope / unscoped-mint caller, so a resource-scoped
    // token can never reach the mint route at all (even to mint something
    // narrower than itself).
    let t = mint_with_caveats(&test_root_key(), &["action = mint", "account = acct-a"]);
    assert_eq!(
        status(&t, "POST", "/v1/auth/tokens").await,
        StatusCode::FORBIDDEN
    );
}
