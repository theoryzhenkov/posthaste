use super::*;

fn ctx(action: Action) -> CaveatContext {
    CaveatContext {
        action: Some(action),
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
    c.action = Some(Action::Send);
    assert!(matches!(
        evaluate_predicate("action = read,tag", &c),
        Decision::Deny(_)
    ));
    assert_eq!(evaluate_predicate("action = send", &c), Decision::Allow);
}

/// A deferred-action context (a `HandlerDerived` route at the perimeter) does
/// not judge `action` caveats — the handler re-check does — while every OTHER
/// caveat axis is still enforced in the same pass.
#[test]
fn deferred_action_context_defers_only_the_action_axis() {
    let mut c = ctx(Action::Read);
    c.action = None;
    assert_eq!(evaluate_predicate("action = tag", &c), Decision::Allow);
    // Resource + expiry caveats keep their full force under a deferred action.
    assert!(matches!(
        evaluate_predicate("account = acct-a", &c),
        Decision::Deny(_)
    ));
    assert!(matches!(
        evaluate_predicate("expires = 2020-01-01T00:00:00Z", &c),
        Decision::Deny(_)
    ));
}

/// The deferred-action escape hatch is reserved for the ONE named-mutation
/// funnel route; any other `HandlerDerived` entry would need its own handler
/// re-check, so growth of this set must be a deliberate, reviewed act.
#[test]
fn handler_derived_action_is_only_the_mutation_route() {
    let handler_derived: Vec<String> = mapped_routes()
        .into_iter()
        .filter_map(|(method, template)| {
            let authz = lookup(method, template).expect("mapped route resolves");
            (authz.action == RouteAction::HandlerDerived).then_some(route_key(method, template))
        })
        .collect();
    assert_eq!(
        handler_derived,
        vec!["POST /runtime/sessions/{session_id}/mutations".to_string()],
        "every HandlerDerived route must have a handler-side per-op authorizer"
    );
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
    assert_eq!(authz.action, RouteAction::Static(Action::Read));
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
