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
fn test_parse_from_alias() {
    let rule = parse_query("f:alice").unwrap();
    assert_eq!(rule.root.operator, SmartMailboxGroupOperator::All);
    assert_eq!(rule.root.nodes.len(), 1);

    assert!(matches!(
        &rule.root.nodes[0],
        SmartMailboxRuleNode::Group(group)
            if group.operator == SmartMailboxGroupOperator::Any && group.nodes.len() == 2
    ));
}

#[test]
fn test_parse_spaced_from_alias_value() {
    let rule = parse_query("Account Creation f: Posthaste Author subject:welcome").unwrap();
    assert_eq!(rule.root.nodes.len(), 4);

    let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[2] else {
        panic!("expected Group node for f: prefix");
    };
    let SmartMailboxRuleNode::Condition(condition) = &group.nodes[0] else {
        panic!("expected FromEmail condition for f: prefix");
    };

    assert_eq!(
        condition.value,
        SmartMailboxValue::String("Posthaste Author".to_string())
    );
}

#[test]
fn test_parse_spaced_subject_value() {
    let rule = parse_query("subject: account creation from:posthaste").unwrap();
    assert_eq!(rule.root.nodes.len(), 2);

    let SmartMailboxRuleNode::Condition(condition) = &rule.root.nodes[0] else {
        panic!("expected Subject condition for subject: prefix");
    };

    assert_eq!(condition.field, SmartMailboxField::Subject);
    assert_eq!(
        condition.value,
        SmartMailboxValue::String("account creation".to_string())
    );
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
fn test_parse_is_read() {
    let rule = parse_query("is:read").unwrap();
    assert_eq!(rule.root.nodes.len(), 1);
    if let SmartMailboxRuleNode::Condition(c) = &rule.root.nodes[0] {
        assert_eq!(c.field, SmartMailboxField::IsRead);
        assert_eq!(c.operator, SmartMailboxOperator::Equals);
        assert_eq!(c.value, SmartMailboxValue::Bool(true));
    } else {
        panic!("expected Condition node for is:read");
    }
}

#[test]
fn test_parse_all_static_state_queries() {
    for query in [
        "is:unread",
        "is:read",
        "is:seen",
        "is:flagged",
        "is:unflagged",
        "is:attachment",
        "is:attachments",
        "has:attachment",
        "has:attachments",
    ] {
        parse_query(query).unwrap_or_else(|error| panic!("{query} failed: {error}"));
    }
}

#[test]
fn test_parse_prefix_and_state_values_case_insensitively() {
    let rule = parse_query("IS:Read FROM: Posthaste").unwrap();
    assert_eq!(rule.root.nodes.len(), 2);
    let SmartMailboxRuleNode::Condition(condition) = &rule.root.nodes[0] else {
        panic!("expected IsRead condition");
    };
    assert_eq!(condition.field, SmartMailboxField::IsRead);
    assert_eq!(condition.value, SmartMailboxValue::Bool(true));
}

#[test]
fn test_parse_mailbox_filter() {
    let rule = parse_query("in:archive").unwrap();
    assert_eq!(rule.root.nodes.len(), 1);
    if let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[0] {
        assert_eq!(group.operator, SmartMailboxGroupOperator::Any);
        assert_eq!(group.nodes.len(), 3);
    } else {
        panic!("expected Group node for in: prefix");
    }
}

#[test]
fn test_parse_source_filter() {
    let rule = parse_query("source: Primary Account").unwrap();
    assert_eq!(rule.root.nodes.len(), 1);
    if let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[0] {
        assert_eq!(group.operator, SmartMailboxGroupOperator::Any);
        assert_eq!(group.nodes.len(), 2);
    } else {
        panic!("expected Group node for source: prefix");
    }
}

#[test]
fn test_parse_id_and_thread_filters() {
    let rule = parse_query("id:message-1 thread:thread-1").unwrap();
    assert_eq!(rule.root.nodes.len(), 2);

    let SmartMailboxRuleNode::Condition(message_id) = &rule.root.nodes[0] else {
        panic!("expected MessageId condition");
    };
    let SmartMailboxRuleNode::Condition(thread_id) = &rule.root.nodes[1] else {
        panic!("expected ThreadId condition");
    };

    assert_eq!(message_id.field, SmartMailboxField::MessageId);
    assert_eq!(thread_id.field, SmartMailboxField::ThreadId);
}

#[test]
fn test_parse_conversation_filter() {
    let rule = parse_query("conversation:conv-1").unwrap();
    let SmartMailboxRuleNode::Condition(condition) = &rule.root.nodes[0] else {
        panic!("expected ConversationId condition");
    };
    assert_eq!(condition.field, SmartMailboxField::ConversationId);
}

#[test]
fn test_rejects_empty_prefixed_value() {
    let error = parse_query("from:").unwrap_err();
    assert!(error.contains("empty value"));

    let error = parse_query("from: is:unread").unwrap_err();
    assert!(error.contains("empty value"));

    let error = parse_query("from: -is:read").unwrap_err();
    assert!(error.contains("empty value"));
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

// -- parse_query_with_scopes: in: scope peeling ---------------------------
// Ported from the deleted `posthaste-authority-server/mail_queries/rules/
// tokenize.rs` so the `in:` extraction behavior is pinned at the one tokenizer
// that now owns it. These exercise the public boundary mail_queries/rules.rs
// consumes; the grammar (quoting, negation, spaced `in:` values) must match
// the deleted duplicate exactly.

#[test]
fn test_with_scopes_extracts_quoted_in_selector() {
    let (rule, scopes) = parse_query_with_scopes("in:\"acct-a/inbox\" from:Alex", &["in"]).unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].value, "acct-a/inbox");
    assert!(!scopes[0].negated);
    let rule = rule.expect("remainder rule for quoted in: + from:");
    assert_eq!(rule.root.nodes.len(), 1);
    let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[0] else {
        panic!("expected from: ANY group");
    };
    assert_eq!(group.operator, SmartMailboxGroupOperator::Any);
    assert_eq!(group.nodes.len(), 2);
}

#[test]
fn test_with_scopes_extracts_spaced_in_selector_until_next_prefix() {
    let (rule, scopes) = parse_query_with_scopes("in:All Mail from:Alex", &["in"]).unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].value, "All Mail");
    assert!(!scopes[0].negated);
    let rule = rule.expect("remainder rule for spaced in: + from:");
    assert_eq!(rule.root.nodes.len(), 1);
    let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[0] else {
        panic!("expected from: ANY group");
    };
    assert_eq!(group.nodes.len(), 2);
}

#[test]
fn test_with_scopes_marks_negated_in_selector() {
    let (rule, scopes) = parse_query_with_scopes("-in:Inbox subject:hello", &["in"]).unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].value, "Inbox");
    assert!(scopes[0].negated);
    let rule = rule.expect("remainder rule for -in: + subject:");
    assert_eq!(rule.root.nodes.len(), 1);
    let SmartMailboxRuleNode::Condition(condition) = &rule.root.nodes[0] else {
        panic!("expected subject Condition");
    };
    assert_eq!(condition.field, SmartMailboxField::Subject);
}

#[test]
fn test_with_scopes_leaves_in_prefix_as_rule_when_not_listed() {
    // parse_query delegates with &[], so `in:` must still become a mailbox
    // node (the behavior the api/bench/store callers rely on).
    let (rule, scopes) = parse_query_with_scopes("in:archive", &[]).unwrap();
    assert!(scopes.is_empty());
    let rule = rule.expect("in:archive yields a rule when not peeled");
    assert_eq!(rule.root.nodes.len(), 1);
    let SmartMailboxRuleNode::Group(group) = &rule.root.nodes[0] else {
        panic!("expected in: mailbox group");
    };
    assert_eq!(group.nodes.len(), 3);
}

#[test]
fn test_with_scopes_returns_none_remainder_for_scope_only_query() {
    // mail_queries/rules.rs relies on `None` here so it does not push an empty
    // remainder rule alongside a scope rule.
    let (rule, scopes) = parse_query_with_scopes("in:inbox", &["in"]).unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].value, "inbox");
    assert!(rule.is_none());
}
