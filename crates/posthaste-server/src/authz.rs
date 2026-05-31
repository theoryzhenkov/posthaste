//! Capability-token authorization: the caveat model, the per-route
//! authorization map, and caveat evaluation. Stage B of the macaroon work —
//! this is where ATTENUATED tokens (carrying first-party caveats) actually
//! restrict access. The authz map is the load-bearing security artifact: a
//! wrong entry grants more than intended, so every route is mapped explicitly
//! and a CI test (`tests/capability_scoping.rs`) cross-checks the map against
//! `openapi.json` so a new, unmapped route fails CI rather than shipping open.
//!
//! # Caveat string format
//!
//! First-party caveats are ASCII predicate strings. Mint/attenuate and verify
//! MUST agree on this exact, documented format. Each caveat is `key = value`
//! (single spaces around `=`):
//!
//! - `action = <verb>[,<verb>...]` — verbs from
//!   `{read, send, tag, move, delete, manage}`. Satisfied iff the route's
//!   required action is in the set.
//! - `account = <source_id>` — satisfied iff the request's account == source_id.
//! - `mailbox = <mailbox_id>` — satisfied iff the request's mailbox == mailbox_id.
//! - `message = <message_id>` — satisfied iff the request's message == message_id.
//! - `expires = <rfc3339-utc>` — satisfied iff `now` < the timestamp.
//!
//! Multiple caveats AND together. An absent caveat is unrestricted on that
//! axis. A caveat the route cannot possibly satisfy (e.g. `account = X` on a
//! request that carries no account dimension, or `account = X` where the
//! request's account is `Y`) FAILS, so the request is denied (403). This is the
//! fail-closed rule: scoping a token to a resource a global endpoint does not
//! expose correctly rejects that token on that endpoint.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use std::collections::HashMap;
use std::sync::OnceLock;

use macaroon::Caveat;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use utoipa::ToSchema;

/// A permitted verb. The action a route represents must be a member of an
/// `action = ...` caveat's set for that caveat to be satisfied. Doubles as the
/// wire enum for the token-mint request (`action = ...` caveats are built from
/// it), so the lowercase serde form matches [`Action::as_str`] exactly — one
/// source of truth for the action vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Read,
    Send,
    Tag,
    Move,
    Delete,
    Manage,
}

impl Action {
    /// The canonical lowercase verb used in `action = ...` caveats.
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Send => "send",
            Action::Tag => "tag",
            Action::Move => "move",
            Action::Delete => "delete",
            Action::Manage => "manage",
        }
    }

    /// Parse a single verb token (used when evaluating an `action` caveat).
    fn parse(token: &str) -> Option<Action> {
        match token {
            "read" => Some(Action::Read),
            "send" => Some(Action::Send),
            "tag" => Some(Action::Tag),
            "move" => Some(Action::Move),
            "delete" => Some(Action::Delete),
            "manage" => Some(Action::Manage),
            _ => None,
        }
    }
}

/// The request-side facts a caveat is evaluated against. `action` is always the
/// route's required action; `account`/`mailbox`/`message` are populated from the
/// matched route's path params (and, for `Filter` routes, the query filter).
/// `None` on an axis means the request has no value on that axis — a caveat
/// restricting that axis then cannot be satisfied and the request is denied.
#[derive(Debug, Clone)]
pub struct CaveatContext {
    pub action: Action,
    pub account: Option<String>,
    pub mailbox: Option<String>,
    pub message: Option<String>,
    /// Current time, for evaluating `expires`.
    pub now: OffsetDateTime,
}

/// Whether a route gates the whole request (resource in the path) or needs a
/// matching query filter to satisfy a resource caveat (aggregate endpoints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMode {
    /// Allow/deny the whole request; the resource identity is in the path.
    Gate,
    /// Aggregate endpoint: an `account`/`mailbox` caveat is satisfied only if
    /// the request carries the matching query filter.
    Filter,
}

/// Which request fields populate the `CaveatContext` for a route. For `Gate`
/// routes these name path params; for `Filter` routes they name query params.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceShape {
    /// Param name that supplies the `account` axis (e.g. `source_id` /
    /// `sourceId`), if the route carries one.
    pub account: Option<&'static str>,
    /// Param name that supplies the `mailbox` axis.
    pub mailbox: Option<&'static str>,
    /// Param name that supplies the `message` axis.
    pub message: Option<&'static str>,
}

impl ResourceShape {
    const fn empty() -> Self {
        Self {
            account: None,
            mailbox: None,
            message: None,
        }
    }
    const fn account(name: &'static str) -> Self {
        Self {
            account: Some(name),
            mailbox: None,
            message: None,
        }
    }
    const fn account_message(account: &'static str, message: &'static str) -> Self {
        Self {
            account: Some(account),
            mailbox: None,
            message: Some(message),
        }
    }
    const fn account_mailbox(account: &'static str, mailbox: &'static str) -> Self {
        Self {
            account: Some(account),
            mailbox: Some(mailbox),
            message: None,
        }
    }
}

/// The authorization descriptor for a single route: the action it represents,
/// which request fields identify its resource, and how resource caveats are
/// applied (gate vs. matching-filter).
#[derive(Debug, Clone, Copy)]
pub struct RouteAuthz {
    pub action: Action,
    pub resource: ResourceShape,
    pub mode: ScopeMode,
}

/// Stable lookup key for the authz map: `METHOD` + the matched route template as
/// the auth middleware sees it (nest-stripped, e.g.
/// `GET /sources/{source_id}/messages`). Method+template is the runtime-stable
/// identity; the completeness test maps every `operationId` to this same key.
pub fn route_key(method: &str, template: &str) -> String {
    format!("{} {}", method.to_ascii_uppercase(), template)
}

/// One entry of the static authz table. Listed as `(method, template, authz)`
/// so it reads as a reviewable security artifact and so the completeness test
/// can confirm every operationId is covered.
struct Entry {
    method: &'static str,
    template: &'static str,
    authz: RouteAuthz,
}

const fn gate(action: Action, resource: ResourceShape) -> RouteAuthz {
    RouteAuthz {
        action,
        resource,
        mode: ScopeMode::Gate,
    }
}

const fn filter(action: Action, resource: ResourceShape) -> RouteAuthz {
    RouteAuthz {
        action,
        resource,
        mode: ScopeMode::Filter,
    }
}

/// The authorization map: every non-exempt `/v1` operation, keyed by
/// method+template (templates are nest-stripped, as the auth middleware sees
/// them). SECURITY ARTIFACT — review each row. `/health`, `/openapi.json`,
/// `/asyncapi.json` are intentionally absent (the perimeter exempts them before
/// the token check); the completeness test treats them as intentionally exempt.
const AUTHZ_TABLE: &[Entry] = &[
    // -- Account / settings / config: management surface, no resource axis the
    //    caveat model can scope (you cannot scope "list all accounts" to one
    //    account), so a scoped token is correctly rejected on these. --
    Entry {
        method: "GET",
        template: "/settings",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "PATCH",
        template: "/settings",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/automation-rules:preview",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/accounts",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/accounts",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // Token mint: derives a narrower capability token. `Manage` + no resource
    // axis means only a full-scope (or `action = manage`, unscoped) caller may
    // mint; the handler attenuates the CALLER's token, so a minted token can
    // only narrow — never widen — the caller's authority.
    Entry {
        method: "POST",
        template: "/auth/tokens",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // Single-account routes: account axis from the `account_id` path param.
    Entry {
        method: "GET",
        template: "/accounts/{account_id}",
        authz: gate(Action::Read, ResourceShape::account("account_id")),
    },
    Entry {
        method: "PATCH",
        template: "/accounts/{account_id}",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "DELETE",
        template: "/accounts/{account_id}",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/verify",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/oauth/start",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/oauth/start",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/oauth/callback",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/enable",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/disable",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/logo",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    // Logo asset is keyed by an opaque image id, not an account/message — a
    // read with no scopable resource axis.
    Entry {
        method: "GET",
        template: "/account-assets/logos/{image_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // -- Sidebar: cross-account aggregate tree, no per-account filter — global
    //    read; an account-scoped token cannot be satisfied here. --
    Entry {
        method: "GET",
        template: "/sidebar",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // -- Smart mailboxes: definitions are global config (Manage to mutate,
    //    Read to view). Their message/conversation LISTS are Filter aggregates. --
    Entry {
        method: "GET",
        template: "/smart-mailboxes",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/smart-mailboxes",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "PATCH",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "DELETE",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/smart-mailboxes:reset-defaults",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // Smart-mailbox MESSAGE list: no `sourceId` query param exists, so it stays a
    // global read (an account caveat is unsatisfiable → such tokens denied).
    // SECURITY: do not add a query axis without first adding + enforcing a source
    // filter param in the handler.
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}/messages",
        authz: filter(Action::Read, ResourceShape::empty()),
    },
    // Smart-mailbox CONVERSATION list: result-side scoped on `sourceId`. The
    // handler ANDs a `source_message_scope_rule` into the smart-mailbox rule in
    // BOTH branches (Tier-1 result-side scoping), so an `account=X` token with a
    // matching `?sourceId=X` sees only that account; a mismatched/absent source
    // makes the caveat unsatisfiable → 403. `mailbox` is intentionally NOT a
    // satisfier here (mailbox ids are not account-unique).
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}/conversations",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    // -- Conversation views. The list is result-side scoped on `sourceId`: the
    //    handler ANDs a `source_message_scope_rule` into the query in BOTH the
    //    search and non-search branches (Tier-1 result-side scoping), so an
    //    `account=X` token with a matching `?sourceId=X` sees only that account;
    //    a mismatched/absent source makes the caveat unsatisfiable → 403.
    //    `mailbox` is intentionally NOT a satisfier (mailbox ids are not
    //    account-unique). A single conversation is addressed by an opaque
    //    conversation id (no scopable account axis in the path), so it is a global
    //    Gate read. SECURITY: keep the handler's source scope in every branch. --
    Entry {
        method: "GET",
        template: "/views/conversations",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "GET",
        template: "/views/conversations/{conversation_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // -- Per-source resources: account axis from `source_id`. --
    Entry {
        method: "GET",
        template: "/sources/{source_id}/mailboxes",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    Entry {
        method: "PATCH",
        template: "/sources/{source_id}/mailboxes/{mailbox_id}",
        authz: gate(
            Action::Manage,
            ResourceShape::account_mailbox("source_id", "mailbox_id"),
        ),
    },
    // Per-source message list: the source is in the path (Gate on account);
    // `mailboxId` is an optional query filter (not a path resource), so this is
    // a Gate, not a Filter — the account axis is exact from the path.
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    // Search is a cross-account aggregate with NO source filter param → global
    // read; an account-scoped token cannot be satisfied (no `sourceId` to match).
    Entry {
        method: "GET",
        template: "/messages/search",
        authz: filter(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sender-addresses",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/identity",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/reply-context",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    // -- Commands: write verbs scoped to the source (and message where present). --
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/send",
        authz: gate(Action::Send, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/set-keywords",
        authz: gate(
            Action::Tag,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/destroy",
        authz: gate(
            Action::Delete,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/sync",
        authz: gate(Action::Manage, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/config:reload",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // SSE event stream: a cross-account read feed. It accepts an `accountId`
    // filter, so it is a Filter aggregate keyed on that query param.
    Entry {
        method: "GET",
        template: "/events",
        authz: filter(
            Action::Read,
            ResourceShape {
                account: Some("accountId"),
                mailbox: Some("mailboxId"),
                message: None,
            },
        ),
    },
];

/// Build the method+template → `RouteAuthz` map once.
fn authz_map() -> &'static HashMap<String, RouteAuthz> {
    static MAP: OnceLock<HashMap<String, RouteAuthz>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::with_capacity(AUTHZ_TABLE.len());
        for entry in AUTHZ_TABLE {
            let key = route_key(entry.method, entry.template);
            debug_assert!(!map.contains_key(&key), "duplicate authz entry for {key}");
            map.insert(key, entry.authz);
        }
        map
    })
}

/// Look up the authz descriptor for a matched route (method + nest-stripped
/// template). `None` means the route is unmapped — the caller must fail CLOSED
/// (treat as misconfiguration, deny) so a new route cannot ship open.
pub fn lookup(method: &str, template: &str) -> Option<RouteAuthz> {
    authz_map().get(&route_key(method, template)).copied()
}

/// Every `(method, template)` pair in the authz table. Used by the completeness
/// test to confirm coverage against the OpenAPI document.
pub fn mapped_routes() -> Vec<(&'static str, &'static str)> {
    AUTHZ_TABLE.iter().map(|e| (e.method, e.template)).collect()
}

/// Outcome of evaluating a token's caveats against a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// All caveats satisfied — allow.
    Allow,
    /// At least one caveat could not be satisfied — deny (403). The string is a
    /// non-sensitive reason for logging/tests.
    Deny(String),
}

/// Evaluate every first-party caveat against the request context. A caveat that
/// is malformed, references an unknown key, or is not satisfied yields `Deny`.
/// `Filter`-route semantics (requiring a matching query filter for resource
/// caveats) are already folded into the `CaveatContext` the caller builds: for
/// a Filter route, the caller populates `account`/`mailbox` from the QUERY
/// params (so a missing/non-matching filter leaves the axis `None` or a
/// different value, and the resource caveat then fails here).
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn evaluate(caveats: &[Caveat], ctx: &CaveatContext) -> Decision {
    for caveat in caveats {
        let Caveat::FirstParty(fp) = caveat else {
            // Third-party caveats are not minted by us and cannot be discharged
            // here — fail closed.
            return Decision::Deny("third-party caveat unsupported".to_string());
        };
        let predicate = fp.predicate();
        let Ok(text) = std::str::from_utf8(predicate.as_ref()) else {
            return Decision::Deny("non-utf8 caveat predicate".to_string());
        };
        if let Decision::Deny(reason) = evaluate_predicate(text, ctx) {
            return Decision::Deny(reason);
        }
    }
    Decision::Allow
}

/// Evaluate one `key = value` predicate string against the context.
fn evaluate_predicate(text: &str, ctx: &CaveatContext) -> Decision {
    let Some((key, value)) = text.split_once('=') else {
        return Decision::Deny(format!("malformed caveat: {text}"));
    };
    let key = key.trim();
    let value = value.trim();
    match key {
        "action" => {
            let allowed = value
                .split(',')
                .any(|tok| Action::parse(tok.trim()).is_some_and(|action| action == ctx.action));
            if allowed {
                Decision::Allow
            } else {
                Decision::Deny(format!(
                    "action {} not permitted by caveat",
                    ctx.action.as_str()
                ))
            }
        }
        "account" => match_axis("account", value, ctx.account.as_deref()),
        "mailbox" => match_axis("mailbox", value, ctx.mailbox.as_deref()),
        "message" => match_axis("message", value, ctx.message.as_deref()),
        "expires" => match OffsetDateTime::parse(value, &Rfc3339) {
            Ok(expiry) => {
                if ctx.now < expiry {
                    Decision::Allow
                } else {
                    Decision::Deny("token expired".to_string())
                }
            }
            Err(_) => Decision::Deny(format!("malformed expires caveat: {value}")),
        },
        other => Decision::Deny(format!("unknown caveat key: {other}")),
    }
}

/// Resource-axis match: the caveat restricts `axis` to `value`; satisfied iff
/// the request's value on that axis equals `value`. A request with NO value on
/// the axis (e.g. an account caveat on a global route, or a Filter route with no
/// matching query filter) cannot satisfy the restriction → deny (fail closed).
fn match_axis(axis: &str, value: &str, request_value: Option<&str>) -> Decision {
    match request_value {
        Some(actual) if actual == value => Decision::Allow,
        Some(_) => Decision::Deny(format!("{axis} out of scope")),
        None => Decision::Deny(format!("{axis} caveat unsatisfiable on this request")),
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(authz_map().len(), AUTHZ_TABLE.len());
    }
}
