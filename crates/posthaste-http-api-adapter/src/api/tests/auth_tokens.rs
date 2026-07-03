use super::*;

fn empty_mint_request() -> CreateAuthTokenRequest {
    CreateAuthTokenRequest {
        actions: None,
        account: None,
        mailbox: None,
        message: None,
        expires_in_seconds: None,
    }
}

fn fixed_now() -> time::OffsetDateTime {
    // 2023-11-14T22:13:20Z
    time::OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
}

// `ApiError` doesn't implement `Debug`, so `Result::unwrap` is unavailable;
// pattern-match to surface the success value.
fn caveats_of(request: &CreateAuthTokenRequest) -> (Vec<String>, Option<String>) {
    match build_token_caveats(request, fixed_now()) {
        Ok(result) => result,
        Err(_) => panic!("expected build_token_caveats to succeed"),
    }
}

#[test]
fn build_token_caveats_empty_request_has_no_caveats() {
    let (predicates, expires_at) = caveats_of(&empty_mint_request());
    assert!(predicates.is_empty());
    assert!(expires_at.is_none());
}

#[test]
fn build_token_caveats_joins_actions_and_axes_in_order() {
    let request = CreateAuthTokenRequest {
        actions: Some(vec![Action::Read, Action::Tag]),
        account: Some("acct-a".into()),
        mailbox: Some("mbx-1".into()),
        message: Some("m9".into()),
        expires_in_seconds: None,
    };
    let (predicates, expires_at) = caveats_of(&request);
    assert_eq!(
        predicates,
        vec![
            "action = read,tag".to_string(),
            "account = acct-a".to_string(),
            "mailbox = mbx-1".to_string(),
            "message = m9".to_string(),
        ]
    );
    assert!(expires_at.is_none());
}

#[test]
fn build_token_caveats_rejects_empty_actions_list() {
    let request = CreateAuthTokenRequest {
        actions: Some(vec![]),
        ..empty_mint_request()
    };
    assert!(build_token_caveats(&request, fixed_now()).is_err());
}

#[test]
fn build_token_caveats_skips_blank_resource_axis() {
    let request = CreateAuthTokenRequest {
        account: Some("   ".into()),
        ..empty_mint_request()
    };
    let (predicates, _) = caveats_of(&request);
    assert!(predicates.is_empty());
}

#[test]
fn build_token_caveats_expiry_is_rfc3339_and_echoed() {
    let request = CreateAuthTokenRequest {
        expires_in_seconds: Some(3600),
        ..empty_mint_request()
    };
    let (predicates, expires_at) = caveats_of(&request);
    let expires_at = expires_at.expect("expiry should be present");
    assert_eq!(predicates, vec![format!("expires = {expires_at}")]);
    assert!(expires_at.starts_with("2023-11-14"));
}

#[test]
fn build_token_caveats_rejects_zero_expiry() {
    let request = CreateAuthTokenRequest {
        expires_in_seconds: Some(0),
        ..empty_mint_request()
    };
    assert!(build_token_caveats(&request, fixed_now()).is_err());
}

fn mint_root() -> crate::token::RootKey {
    crate::token::RootKey::from_test_bytes([7u8; 32])
}

fn derived(caller: Option<String>, predicates: &[&str]) -> Vec<macaroon::Caveat> {
    let root = mint_root();
    let preds: Vec<String> = predicates.iter().map(|p| p.to_string()).collect();
    let token = match derive_capability_token(caller, &root, &preds) {
        Ok(token) => token,
        Err(_) => panic!("derive_capability_token should succeed"),
    };
    crate::token::verify_authenticity(&token, &root).expect("minted token is authentic")
}

fn ctx(action: Action) -> crate::authz::CaveatContext {
    crate::authz::CaveatContext {
        action,
        account: None,
        mailbox: None,
        message: None,
        now: fixed_now(),
    }
}

#[test]
fn derive_capability_token_attenuates_caller_to_requested_scope() {
    // A full-scope caller minting `action = read` gets a token that reads but
    // is denied on a write verb — proving the caveat was applied.
    let caller = crate::token::mint_full_scope_token(&mint_root());
    let caveats = derived(Some(caller), &["action = read"]);
    assert_eq!(caveats.len(), 1);
    assert_eq!(
        crate::authz::evaluate(&caveats, &ctx(Action::Read)),
        crate::authz::Decision::Allow
    );
    assert!(matches!(
        crate::authz::evaluate(&caveats, &ctx(Action::Send)),
        crate::authz::Decision::Deny(_)
    ));
}

#[test]
fn derive_capability_token_cannot_widen_a_scoped_caller() {
    // A read-only caller requesting `action = manage` cannot escalate: the
    // result carries BOTH caveats (they AND), so it acts as neither manage
    // (read caveat denies) nor anything else — never wider than the caller.
    let read_only = crate::token::attenuate(
        &crate::token::mint_full_scope_token(&mint_root()),
        "action = read",
    )
    .expect("attenuation should succeed");
    let caveats = derived(Some(read_only), &["action = manage"]);
    assert!(matches!(
        crate::authz::evaluate(&caveats, &ctx(Action::Manage)),
        crate::authz::Decision::Deny(_)
    ));
}

#[test]
fn derive_capability_token_mints_from_root_without_a_caller() {
    // `require_auth` off: no caller token, so mint from the root key with the
    // requested caveats.
    let caveats = derived(None, &["action = read", "account = acct-a"]);
    assert_eq!(caveats.len(), 2);
    // The account caveat is present and enforced: a request with no account
    // axis cannot satisfy it, even though the action matches.
    assert!(matches!(
        crate::authz::evaluate(&caveats, &ctx(Action::Read)),
        crate::authz::Decision::Deny(_)
    ));
}

#[test]
fn derive_capability_token_escalates_from_a_mint_only_caller() {
    // The discovery bootstrap shape (RFC-L2-scripting §7 ruling 11): a caller
    // scoped to exactly `mint` + `read` has no write scope of its own to
    // narrow FROM, so naive attenuation could never produce a write-capable
    // token (two `action = ...` caveats AND together — an unsatisfiable
    // intersection). Because the caller GRANTS `mint`, the route mints FRESH
    // from the root key instead: the result carries ONLY the newly requested
    // caveat, not the caller's own `action = mint,read` stacked underneath.
    let root = mint_root();
    let bootstrap = crate::token::mint_with_caveats(&root, &["action = mint,read"]);
    let caveats = derived(Some(bootstrap), &["action = tag,move,delete"]);
    assert_eq!(
        caveats.len(),
        1,
        "fresh mint carries only the requested caveat, not the caller's own"
    );
    assert_eq!(
        crate::authz::evaluate(&caveats, &ctx(Action::Tag)),
        crate::authz::Decision::Allow
    );
    // The escalated token is NOT itself mint-carrying (it was never asked for
    // `mint`) — it cannot chain-mint further.
    assert!(matches!(
        crate::authz::evaluate(&caveats, &ctx(Action::Mint)),
        crate::authz::Decision::Deny(_)
    ));
}

#[test]
fn derive_capability_token_non_mint_caller_still_only_narrows() {
    // A caller that does NOT hold `mint` (e.g. `action = manage` alone) is
    // unaffected by the escalation path — same narrowing behavior as always.
    // (This caller could never reach the real route post-ruling-11 either,
    // since the route now gates on `Mint`; this exercises the function-level
    // invariant directly, same as `derive_capability_token_cannot_widen_a_scoped_caller`.)
    let manage_only = crate::token::mint_with_caveats(&mint_root(), &["action = manage"]);
    let caveats = derived(Some(manage_only), &["action = tag"]);
    assert_eq!(caveats.len(), 2, "attenuation stacks, it does not replace");
    assert!(matches!(
        crate::authz::evaluate(&caveats, &ctx(Action::Tag)),
        crate::authz::Decision::Deny(_)
    ));
}
