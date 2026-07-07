//! The one query grammar: a text parser that compiles human-readable search
//! strings into [`MailQueryRule`] trees.
//!
//! Syntax: `prefix:value` tokens separated by whitespace. Quoted values
//! (`"hello world"`), negation (`-prefix:value`), and aliases such as `f:` for
//! `from:` are supported. [`parse_query_with_scopes`] peels a caller-chosen
//! set of prefixes (e.g. `in:`) out of the tree for service-side resolution
//! using the one tokenizer this crate owns — there is no second tokenizer
//! (D28).
//!
//! Extracted out of `posthaste-domain-service` (RFC-L2-scripting §7 ruling 4)
//! so both smart mailboxes (domain-service) and the rules engine's WHEN-clause
//! grammar (a later unit) consume the same parser without the rules engine
//! dragging in domain-service. Depends on `posthaste-domain-model` only (the
//! `MailQueryRule`/node output types live there) and is wasm-pure: no
//! tokio, no http, nothing that would break a wasm32 target build.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use posthaste_domain_model::{
    MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator, MailQueryOperator,
    MailQueryRule, MailQueryRuleNode, MailQueryValue,
};

mod date;
mod nodes;
mod tokenizer;

use nodes::parse_token;
use tokenizer::tokenize;

/// A token whose prefix matched the caller's scope allowlist, returned unparsed
/// by [`parse_query_with_scopes`] so the caller can resolve it against
/// out-of-grammar state (e.g. an `in:` selector that names a mailbox scope or a
/// saved smart mailbox, which the service-free parser cannot resolve itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeToken {
    pub negated: bool,
    pub value: String,
}

/// Parses a human-readable query string into a [`MailQueryRule`].
///
/// Returns `Err` with a description when the query contains a malformed token
/// (e.g. unknown `is:` value, unparseable date, bad `newer:`/`older:` unit).
///
/// `in:` (and every other recognised prefix) is parsed into a rule node here.
/// Callers that need to peel `in:` scope selectors out for service-side
/// resolution should use [`parse_query_with_scopes`].
pub fn parse_query(query: &str) -> Result<MailQueryRule, String> {
    let (rule, _scopes) = parse_query_with_scopes(query, &[])?;
    Ok(rule.unwrap_or_else(empty_rule))
}

/// Parses a query, peeling tokens whose prefix is in `scope_prefixes`
/// (matched case-insensitively) out of the rule tree and returning them as
/// [`ScopeToken`]s for the caller to resolve.
///
/// The remaining tokens are parsed with exactly the same grammar as
/// [`parse_query`] — quoting, negation, and spaced-value handling are
/// identical — but tokenized once rather than re-feeding a reconstructed
/// remainder string, so the two paths cannot drift.
///
/// Returns `None` for the rule when no non-scope tokens remain, so a caller
/// that only emits a rule for real content can distinguish `in:only` from
/// `in:only free text` without inspecting rule internals.
pub fn parse_query_with_scopes(
    query: &str,
    scope_prefixes: &[&str],
) -> Result<(Option<MailQueryRule>, Vec<ScopeToken>), String> {
    let tokens = tokenize(query);
    let mut nodes: Vec<MailQueryRuleNode> = Vec::new();
    let mut scopes: Vec<ScopeToken> = Vec::new();

    for token in tokens {
        if is_scope_prefix(token.prefix.as_deref(), scope_prefixes) {
            scopes.push(ScopeToken {
                negated: token.negated,
                value: token.value,
            });
            continue;
        }
        nodes.extend(parse_token(&token)?);
    }

    let rule = if nodes.is_empty() {
        None
    } else {
        Some(MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes,
            },
        })
    };
    Ok((rule, scopes))
}

fn is_scope_prefix(prefix: Option<&str>, scope_prefixes: &[&str]) -> bool {
    let Some(prefix) = prefix else { return false };
    scope_prefixes
        .iter()
        .any(|scope| scope.eq_ignore_ascii_case(prefix))
}

fn empty_rule() -> MailQueryRule {
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests;
