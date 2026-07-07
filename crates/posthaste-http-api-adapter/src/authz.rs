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
//!   `{read, send, tag, move, delete, manage, mint}`. Satisfied iff the route's
//!   required action is in the set. `mint` gates ONLY `POST /v1/auth/tokens`
//!   (RFC-L2-scripting §7 ruling 11) and is treated as an ISSUANCE right, not a
//!   substantive scope: see `derive_capability_token` in `api::auth_tokens` for
//!   why a caller holding `mint` can obtain tokens WIDER than its own scope
//!   (the discovery bootstrap is `{mint, tap:read}` yet can mint write-capable
//!   tokens) while every other verb still only ever narrows.
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

use macaroon::Caveat;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use utoipa::ToSchema;

mod operation;
mod route_table;

pub(crate) use operation::{required_actions, OperationActions};
#[cfg(test)]
use route_table::{authz_entry_count, authz_map};
pub use route_table::{lookup, mapped_routes};

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
    /// The right to call `POST /v1/auth/tokens` (mint/attenuate capability
    /// tokens). Gates ONLY that route — see the module doc's caveat-format
    /// section for why this verb is an issuance right rather than a
    /// substantive scope.
    Mint,
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
            Action::Mint => "mint",
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
            "mint" => Some(Action::Mint),
            _ => None,
        }
    }
}

/// The request-side facts a caveat is evaluated against. `action` is the
/// required action — the route's static action, or, for a
/// [`RouteAction::HandlerDerived`] route evaluated at the PERIMETER, `None`:
/// the action axis is deferred to the handler's per-operation re-check (an
/// `action = ...` caveat then evaluates as satisfied *here* precisely because
/// the handler re-evaluates with the derived action before dispatch — see
/// `api::runtime_stream::mutations`). Handlers must always pass `Some`.
/// `account`/`mailbox`/`message` are populated from the matched route's path
/// params (and, for `Filter` routes, the query filter). `None` on an axis means
/// the request has no value on that axis — a caveat restricting that axis then
/// cannot be satisfied and the request is denied.
#[derive(Debug, Clone)]
pub struct CaveatContext {
    pub action: Option<Action>,
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

/// The action a route requires: fixed by the route shape, or derived
/// per-request from the BODY by the route's handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAction {
    /// The route represents exactly one action, known statically.
    Static(Action),
    /// The required action depends on the request body (the named-mutation
    /// funnel, where one route carries ops from `setKeywords` to `destroy` and
    /// `send`). The perimeter middleware DEFERS the action axis — resource and
    /// expiry caveats are still enforced there — and the route's handler MUST
    /// derive and enforce the per-operation action (deny-by-default) before
    /// dispatch. See [`required_actions`] and `api::runtime_stream::mutations`.
    /// The pairing is pinned by `handler_derived_action_is_only_the_mutation_route`
    /// in the authz tests.
    HandlerDerived,
}

/// The authorization descriptor for a single route: the action it represents,
/// which request fields identify its resource, and how resource caveats are
/// applied (gate vs. matching-filter).
#[derive(Debug, Clone, Copy)]
pub struct RouteAuthz {
    pub action: RouteAction,
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
            // A `None` action is a `RouteAction::HandlerDerived` route evaluated
            // at the perimeter: the action axis is DEFERRED to the handler's
            // per-operation re-check (which always evaluates with `Some`), so
            // the caveat is not judged here. This never weakens enforcement:
            // only the one handler-derived route builds a `None` context, and
            // its handler re-evaluates every caveat with the derived action
            // before dispatch.
            let Some(required) = ctx.action else {
                return Decision::Allow;
            };
            let allowed = value
                .split(',')
                .any(|tok| Action::parse(tok.trim()).is_some_and(|action| action == required));
            if allowed {
                Decision::Allow
            } else {
                Decision::Deny(format!(
                    "action {} not permitted by caveat",
                    required.as_str()
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
mod tests;
