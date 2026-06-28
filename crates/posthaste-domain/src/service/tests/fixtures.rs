use super::*;

pub(super) fn sample_smart_mailbox() -> SmartMailbox {
    SmartMailbox {
        id: SmartMailboxId::from("default-inbox"),
        name: "Inbox".to_string(),
        position: 0,
        kind: SmartMailboxKind::Default,
        default_key: Some("inbox".to_string()),
        role: None,
        parent_id: None,
        rule: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::MailboxRole,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String("inbox".to_string()),
                })],
            },
        },
        created_at: crate::RFC3339_EPOCH.to_string(),
        updated_at: crate::RFC3339_EPOCH.to_string(),
    }
}

pub(super) fn sample_source() -> AccountSettings {
    AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: crate::AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: Default::default(),
        created_at: crate::RFC3339_EPOCH.to_string(),
        updated_at: crate::RFC3339_EPOCH.to_string(),
    }
}

pub(super) fn sample_message_summary(id: &str, keywords: Vec<String>) -> MessageSummary {
    MessageSummary {
        id: MessageId::from(id),
        source_id: AccountId::from("primary"),
        source_name: "Primary".to_string(),
        source_thread_id: ThreadId::from("thread-1"),
        conversation_id: ConversationId::from("conversation-1"),
        subject: Some("Hello".to_string()),
        from_name: Some("PostHaste Updates".to_string()),
        from_email: Some("hello@example.com".to_string()),
        to: Vec::new(),
        preview: None,
        received_at: crate::RFC3339_EPOCH.to_string(),
        has_attachment: false,
        is_read: false,
        is_flagged: false,
        mailbox_ids: vec![MailboxId::from("inbox")],
        keywords,
        version: None,
    }
}

pub(super) fn sample_message_record(id: &str, size: i64, has_attachment: bool) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from("thread-1"),
        subject: Some("Hello".to_string()),
        from_name: Some("PostHaste Updates".to_string()),
        from_email: Some("hello@example.com".to_string()),
        received_at: crate::RFC3339_EPOCH.to_string(),
        has_attachment,
        size,
        mailbox_ids: vec![MailboxId::from("inbox")],
        ..Default::default()
    }
}

pub(super) fn sample_cache_fetch_candidate(
    message_id: &str,
    fetch_bytes: u64,
) -> CacheFetchCandidate {
    CacheFetchCandidate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        fetch_bytes,
        priority: 1.0,
    }
}

pub(super) fn sample_fetch_lease(request_limit: usize, byte_limit: u64) -> CacheFetchLease {
    CacheFetchLease::new(request_limit, byte_limit, 0.0)
}

pub(super) fn sample_cache_rescore_candidate(message_id: &str) -> CacheRescoreCandidate {
    CacheRescoreCandidate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        state: CacheObjectState::Wanted,
        value_bytes: 32 * 1024,
        fetch_bytes: 32 * 1024,
        priority: 1.0,
        message_size: 32 * 1024,
        has_attachment: false,
        received_at: crate::RFC3339_EPOCH.to_string(),
        in_inbox: true,
        unread: true,
        flagged: false,
        thread_activity: 0.0,
        sender_affinity: 0.0,
        local_behavior: 0.0,
        search: Some(crate::CacheSearchSignals {
            total_messages: 1_000,
            result_count: 5,
            result_rank: 0,
        }),
        direct_user_boost: 0.8,
        pinned: false,
        signal_reason: "search-visible".to_string(),
        rescore_priority: 108.0,
    }
}

pub(super) fn sample_fetched_body() -> FetchedBody {
    FetchedBody {
        body_html: None,
        body_text: Some("Cached body".to_string()),
        raw_mime: None,
        attachments: Vec::new(),
    }
}

pub(super) fn sample_automation_rule() -> AutomationRule {
    AutomationRule {
        id: "rule-posthaste".to_string(),
        name: "Posthaste".to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::Any,
                negated: false,
                nodes: vec![
                    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::FromName,
                        operator: SmartMailboxOperator::Contains,
                        negated: false,
                        value: SmartMailboxValue::String("posthaste".to_string()),
                    }),
                    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::FromEmail,
                        operator: SmartMailboxOperator::Contains,
                        negated: false,
                        value: SmartMailboxValue::String("posthaste".to_string()),
                    }),
                ],
            },
        },
        actions: vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
        backfill: true,
    }
}
