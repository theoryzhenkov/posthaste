//! Query text parser that compiles human-readable search strings into
//! [`SmartMailboxRule`] trees.
//!
//! Syntax: `prefix:value` tokens separated by whitespace. Quoted values
//! (`"hello world"`) and negation (`-prefix:value`) are supported.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::{
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
};

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

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

struct Token {
    negated: bool,
    prefix: Option<String>,
    value: String,
}

/// Splits input on whitespace, respecting `"quoted strings"` and `-` negation.
fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        let negated = chars[i] == '-' && i + 1 < len && !chars[i + 1].is_whitespace();
        if negated {
            i += 1;
        }

        // scan for prefix (everything before the first ':')
        let start = i;
        let mut colon_pos = None;
        while i < len && !chars[i].is_whitespace() {
            if chars[i] == ':' && colon_pos.is_none() {
                colon_pos = Some(i);
                break;
            }
            i += 1;
        }

        if let Some(cp) = colon_pos {
            let prefix: String = chars[start..cp].iter().collect();
            i = cp + 1; // skip ':'

            let value = scan_value(&chars, &mut i);
            tokens.push(Token {
                negated,
                prefix: Some(prefix),
                value,
            });
        } else {
            // no colon -- this is free text; rescan from `start`
            i = start;
            let value = scan_value(&chars, &mut i);
            tokens.push(Token {
                negated,
                prefix: None,
                value,
            });
        }
    }

    tokens
}

/// Reads a value starting at `chars[*pos]`. Handles quoted strings.
fn scan_value(chars: &[char], pos: &mut usize) -> String {
    let len = chars.len();
    if *pos < len && chars[*pos] == '"' {
        // quoted value
        *pos += 1; // skip opening quote
        let start = *pos;
        while *pos < len && chars[*pos] != '"' {
            *pos += 1;
        }
        let value: String = chars[start..*pos].iter().collect();
        if *pos < len {
            *pos += 1; // skip closing quote
        }
        value
    } else {
        let start = *pos;
        while *pos < len && !chars[*pos].is_whitespace() {
            *pos += 1;
        }
        chars[start..*pos].iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Token -> SmartMailboxRuleNode mapping
// ---------------------------------------------------------------------------

fn parse_token(token: &Token) -> Result<Vec<SmartMailboxRuleNode>, String> {
    match token.prefix.as_deref() {
        Some(prefix) => parse_prefixed(prefix, &token.value, token.negated),
        None => Ok(vec![free_text_node(&token.value, token.negated)]),
    }
}

fn parse_prefixed(
    prefix: &str,
    value: &str,
    negated: bool,
) -> Result<Vec<SmartMailboxRuleNode>, String> {
    match prefix {
        "from" => Ok(vec![from_node(value, negated)]),
        "subject" => Ok(vec![condition_node(
            SmartMailboxField::Subject,
            SmartMailboxOperator::Contains,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        "is" => is_node(value, negated),
        "has" => has_node(value, negated),
        "tag" => Ok(vec![condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        "before" => Ok(vec![condition_node(
            SmartMailboxField::ReceivedAt,
            SmartMailboxOperator::Before,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        "after" => Ok(vec![condition_node(
            SmartMailboxField::ReceivedAt,
            SmartMailboxOperator::After,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        "date" => date_node(value, negated),
        "newer" => relative_date_node(value, SmartMailboxOperator::After, negated),
        "older" => relative_date_node(value, SmartMailboxOperator::Before, negated),
        _ => Err(format!("unknown search prefix: {prefix}")),
    }
}

// -- helpers ----------------------------------------------------------------

fn condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
    negated: bool,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated,
        value,
    })
}

/// `from:value` -> ANY(FromEmail contains, FromName contains)
fn from_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromEmail,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}

/// `is:unread` / `is:flagged`
fn is_node(value: &str, negated: bool) -> Result<Vec<SmartMailboxRuleNode>, String> {
    match value {
        "unread" => Ok(vec![condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
            negated,
        )]),
        "flagged" => Ok(vec![condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown is: value: {value}")),
    }
}

/// `has:attachment`
fn has_node(value: &str, negated: bool) -> Result<Vec<SmartMailboxRuleNode>, String> {
    match value {
        "attachment" => Ok(vec![condition_node(
            SmartMailboxField::HasAttachment,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown has: value: {value}")),
    }
}

/// `date:YYYY-MM-DD` -> OnOrAfter start-of-day AND Before start-of-next-day.
fn date_node(value: &str, negated: bool) -> Result<Vec<SmartMailboxRuleNode>, String> {
    let date = time::Date::parse(
        value,
        &time::format_description::parse("[year]-[month]-[day]")
            .map_err(|e| format!("date format error: {e}"))?,
    )
    .map_err(|e| format!("invalid date '{value}': {e}"))?;

    let start = date
        .midnight()
        .assume_utc()
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))?;

    let next_day = date.next_day().ok_or_else(|| "date overflow".to_string())?;
    let end = next_day
        .midnight()
        .assume_utc()
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))?;

    Ok(vec![SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::All,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::ReceivedAt,
                operator: SmartMailboxOperator::OnOrAfter,
                negated: false,
                value: SmartMailboxValue::String(start),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::ReceivedAt,
                operator: SmartMailboxOperator::Before,
                negated: false,
                value: SmartMailboxValue::String(end),
            }),
        ],
    })])
}

/// `newer:7d` / `older:2w` — relative date from now.
fn relative_date_node(
    value: &str,
    operator: SmartMailboxOperator,
    negated: bool,
) -> Result<Vec<SmartMailboxRuleNode>, String> {
    let iso = compute_relative_date(value)?;
    Ok(vec![condition_node(
        SmartMailboxField::ReceivedAt,
        operator,
        SmartMailboxValue::String(iso),
        negated,
    )])
}

/// Parses `Nd`, `Nw`, `Nm`, `Ny` and subtracts from now.
fn compute_relative_date(spec: &str) -> Result<String, String> {
    if spec.len() < 2 {
        return Err(format!("invalid relative date: {spec}"));
    }
    let (num_str, unit) = spec.split_at(spec.len() - 1);
    let n: i64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in relative date: {num_str}"))?;

    let now = OffsetDateTime::now_utc();
    let target = match unit {
        "d" => now - Duration::days(n),
        "w" => now - Duration::weeks(n),
        "m" => now - Duration::days(n * 30),
        "y" => now - Duration::days(n * 365),
        _ => return Err(format!("unknown relative date unit: {unit}")),
    };
    target
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))
}

/// Free text: search across FromName, FromEmail, Subject, Preview.
fn free_text_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromEmail,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::Subject,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::Preview,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_from_prefix() {
        let rule = parse_query("from:alice").unwrap();
        assert_eq!(rule.root.operator, SmartMailboxGroupOperator::All);
        assert_eq!(rule.root.nodes.len(), 1);

        // from: produces an ANY group with two conditions
        let node = &rule.root.nodes[0];
        if let SmartMailboxRuleNode::Group(g) = node {
            assert_eq!(g.operator, SmartMailboxGroupOperator::Any);
            assert!(!g.negated);
            assert_eq!(g.nodes.len(), 2);
        } else {
            panic!("expected Group node for from: prefix");
        }
    }

    #[test]
    fn test_parse_free_text() {
        let rule = parse_query("hello").unwrap();
        assert_eq!(rule.root.nodes.len(), 1);
        if let SmartMailboxRuleNode::Group(g) = &rule.root.nodes[0] {
            assert_eq!(g.operator, SmartMailboxGroupOperator::Any);
            assert_eq!(g.nodes.len(), 4); // FromName, FromEmail, Subject, Preview
        } else {
            panic!("expected Group node for free text");
        }
    }

    #[test]
    fn test_parse_is_unread() {
        let rule = parse_query("is:unread").unwrap();
        assert_eq!(rule.root.nodes.len(), 1);
        if let SmartMailboxRuleNode::Condition(c) = &rule.root.nodes[0] {
            assert_eq!(c.field, SmartMailboxField::IsRead);
            assert_eq!(c.operator, SmartMailboxOperator::Equals);
            assert_eq!(c.value, SmartMailboxValue::Bool(false));
            assert!(!c.negated);
        } else {
            panic!("expected Condition node for is:unread");
        }
    }

    #[test]
    fn test_parse_negation() {
        let rule = parse_query("-from:bob").unwrap();
        assert_eq!(rule.root.nodes.len(), 1);
        if let SmartMailboxRuleNode::Group(g) = &rule.root.nodes[0] {
            assert!(g.negated);
            assert_eq!(g.operator, SmartMailboxGroupOperator::Any);
        } else {
            panic!("expected negated Group node");
        }
    }

    #[test]
    fn test_parse_quoted_string() {
        let rule = parse_query("subject:\"weekly report\"").unwrap();
        assert_eq!(rule.root.nodes.len(), 1);
        if let SmartMailboxRuleNode::Condition(c) = &rule.root.nodes[0] {
            assert_eq!(c.field, SmartMailboxField::Subject);
            assert_eq!(
                c.value,
                SmartMailboxValue::String("weekly report".to_string())
            );
        } else {
            panic!("expected Condition node for quoted subject");
        }
    }

    #[test]
    fn test_parse_multiple_tokens() {
        let rule = parse_query("from:alice is:unread subject:test").unwrap();
        assert_eq!(rule.root.nodes.len(), 3);
        assert_eq!(rule.root.operator, SmartMailboxGroupOperator::All);
    }

    #[test]
    fn test_parse_empty_query() {
        let rule = parse_query("").unwrap();
        assert!(rule.root.nodes.is_empty());
    }
}
