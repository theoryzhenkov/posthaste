//! Query text parser that compiles human-readable search strings into
//! [`SmartMailboxRule`] trees.
//!
//! Syntax: `prefix:value` tokens separated by whitespace. Quoted values
//! (`"hello world"`), negation (`-prefix:value`), and aliases such as `f:` for
//! `from:` are supported.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::{
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
};

mod date;
mod nodes;
mod tokenizer;

use nodes::parse_token;
use tokenizer::tokenize;

/// Parses a human-readable query string into a [`SmartMailboxRule`].
///
/// Returns `Err` with a description when the query contains a malformed token
/// (e.g. unknown `is:` value, unparseable date, bad `newer:`/`older:` unit).
pub fn parse_query(query: &str) -> Result<SmartMailboxRule, String> {
    let tokens = tokenize(query);
    let mut nodes: Vec<SmartMailboxRuleNode> = Vec::new();

    for token in tokens {
        let parsed = parse_token(&token)?;
        nodes.extend(parsed);
    }

    Ok(SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    })
}

#[cfg(test)]
mod tests;
