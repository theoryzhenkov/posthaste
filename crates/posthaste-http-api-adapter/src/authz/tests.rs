use super::*;

fn ctx(action: Action) -> CaveatContext {
    CaveatContext {
        action,
        account: None,
        mailbox: None,
        message: None,
        now: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
    }
}

#[test]
fn action_caveat_allows_listed_and_denies_unlisted() {
    let mut c = ctx(Action::Read);
    assert_eq!(evaluate_predicate("action = read", &c), Decision::Allow);
    assert_eq!(evaluate_predicate("action = read,tag", &c), Decision::Allow);
    c.action = Action::Send;
    assert!(matches!(
        evaluate_predicate("action = read,tag", &c),
        Decision::Deny(_)
    ));
    assert_eq!(evaluate_predicate("action = send", &c), Decision::Allow);
}

#[test]
fn account_caveat_matches_path_value() {
    let mut c = ctx(Action::Read);
    c.account = Some("acct-a".to_string());
    assert_eq!(evaluate_predicate("account = acct-a", &c), Decision::Allow);
    assert!(matches!(
        evaluate_predicate("account = acct-b", &c),
        Decision::Deny(_)
    ));
}

#[test]
fn account_caveat_on_request_without_account_is_denied() {
    // Global route (no account axis) + account-scoped token → unsatisfiable.
    let c = ctx(Action::Read);
    assert!(matches!(
        evaluate_predicate("account = acct-a", &c),
        Decision::Deny(_)
    ));
}

#[test]
fn message_caveat_matches_and_rejects() {
    let mut c = ctx(Action::Read);
    c.message = Some("msg-1".to_string());
    assert_eq!(evaluate_predicate("message = msg-1", &c), Decision::Allow);
    assert!(matches!(
        evaluate_predicate("message = msg-2", &c),
        Decision::Deny(_)
    ));
}

#[test]
fn expires_caveat_past_and_future() {
    let c = ctx(Action::Read); // now = 2023-11-14T...
    assert!(matches!(
        evaluate_predicate("expires = 2020-01-01T00:00:00Z", &c),
        Decision::Deny(_)
    ));
    assert_eq!(
        evaluate_predicate("expires = 2099-01-01T00:00:00Z", &c),
        Decision::Allow
    );
    assert!(matches!(
        evaluate_predicate("expires = not-a-date", &c),
        Decision::Deny(_)
    ));
}

#[test]
fn malformed_and_unknown_caveats_deny() {
    let c = ctx(Action::Read);
    assert!(matches!(
        evaluate_predicate("no-equals-sign", &c),
        Decision::Deny(_)
    ));
    assert!(matches!(
        evaluate_predicate("bogus = x", &c),
        Decision::Deny(_)
    ));
}

#[test]
fn lookup_resolves_a_known_route() {
    let authz = lookup("GET", "/sources/{source_id}/messages/{message_id}")
        .expect("mapped route should resolve");
    assert_eq!(authz.action, Action::Read);
    assert_eq!(authz.resource.account, Some("source_id"));
    assert_eq!(authz.resource.message, Some("message_id"));
    assert_eq!(authz.mode, ScopeMode::Gate);
}

#[test]
fn lookup_unmapped_route_is_none() {
    assert!(lookup("GET", "/totally/unmapped").is_none());
}

#[test]
fn authz_table_has_no_duplicate_keys() {
    // Building the map debug-asserts uniqueness; force it here.
    assert_eq!(authz_map().len(), authz_entry_count());
}
