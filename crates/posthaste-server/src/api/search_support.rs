use super::*;

pub(super) fn parse_optional_search_rule(
    query: Option<&str>,
) -> Result<Option<SmartMailboxRule>, ApiError> {
    let Some(query) = query else {
        return Ok(None);
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    posthaste_domain::search::parse_query(query)
        .map(Some)
        .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, ApiErrorCode::InvalidQuery, msg))
}

fn rule_condition(field: SmartMailboxField, value: impl Into<String>) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator: SmartMailboxOperator::Equals,
        negated: false,
        value: SmartMailboxValue::String(value.into()),
    })
}

fn all_rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

pub(super) fn combine_rules(rules: Vec<SmartMailboxRule>) -> SmartMailboxRule {
    all_rule(
        rules
            .into_iter()
            .map(|rule| SmartMailboxRuleNode::Group(rule.root))
            .collect(),
    )
}

pub(super) fn source_message_scope_rule(
    source_id: &str,
    mailbox_id: Option<&MailboxId>,
) -> SmartMailboxRule {
    let mut nodes = vec![rule_condition(SmartMailboxField::SourceId, source_id)];
    if let Some(mailbox_id) = mailbox_id {
        nodes.push(rule_condition(
            SmartMailboxField::MailboxId,
            mailbox_id.as_str(),
        ));
    }
    all_rule(nodes)
}

async fn record_search_cache_visibility(
    state: &Arc<AppState>,
    page: &MessagePage,
    scope_rule: &SmartMailboxRule,
    result_rule: &SmartMailboxRule,
    operation_id: Option<&str>,
) {
    let total_messages = match state.service.count_messages_by_rule(scope_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(
                events::CACHE_SEARCH_VISIBILITY_SCOPE_COUNT_FAILED,
                error = %error,
                "skipping cache search visibility signals because scope count failed"
            );
            return;
        }
    };
    let result_count = match state.service.count_messages_by_rule(result_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(
                events::CACHE_SEARCH_VISIBILITY_RESULT_COUNT_FAILED,
                error = %error,
                "skipping cache search visibility signals because result count failed"
            );
            return;
        }
    };
    let account_ids =
        match state
            .service
            .record_cache_search_visibility(page, total_messages, result_count)
        {
            Ok(account_ids) => account_ids,
            Err(error) => {
                ph_warn!(
                    events::CACHE_SEARCH_VISIBILITY_RECORD_FAILED,
                    error = %error,
                    "failed to record cache search visibility signals"
                );
                return;
            }
        };
    for account_id in account_ids {
        if let Err(error) = state
            .supervisor
            .trigger_cache_maintenance(&account_id, operation_id.map(str::to_string))
            .await
        {
            ph_warn!(
                events::CACHE_MAINTENANCE_TRIGGER_FAILED,
                account_id = %account_id,
                error = %error,
                "failed to trigger cache maintenance after search visibility signal"
            );
        }
    }
}

pub(super) fn spawn_search_cache_visibility(
    state: Arc<AppState>,
    page: MessagePage,
    scope_rule: SmartMailboxRule,
    result_rule: SmartMailboxRule,
    operation_id: Option<String>,
) {
    tokio::spawn(async move {
        record_search_cache_visibility(
            &state,
            &page,
            &scope_rule,
            &result_rule,
            operation_id.as_deref(),
        )
        .await;
    });
}
