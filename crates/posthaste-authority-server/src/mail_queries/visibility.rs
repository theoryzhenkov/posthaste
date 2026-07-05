use std::sync::Arc;

use posthaste_contract_core::SearchVisibilityRequest;
use posthaste_domain_model::{AccountId, MessagePage, SmartMailboxRule};
use posthaste_domain_service::MailService;
use posthaste_observability::{events, ph_warn};

use crate::supervisor::AccountSupervisor;

pub(crate) async fn record(
    service: &Arc<MailService>,
    supervisor: &Arc<AccountSupervisor>,
    result_query: &str,
    page: &MessagePage,
    request: SearchVisibilityRequest,
) {
    let scope_rule = match super::rules::compile(service, &request.base_query) {
        Ok(rule) => rule,
        Err(error) => {
            ph_warn!(events::CACHE_SEARCH_VISIBILITY_SCOPE_COUNT_FAILED, error = %error, "cache visibility skipped because base query failed");
            return;
        }
    };
    let result_rule = match super::rules::compile(service, result_query) {
        Ok(rule) => rule,
        Err(error) => {
            ph_warn!(events::CACHE_SEARCH_VISIBILITY_RESULT_COUNT_FAILED, error = %error, "cache visibility skipped because result query failed");
            return;
        }
    };
    let Some((total_messages, result_count)) = counts(service, &scope_rule, &result_rule) else {
        return;
    };
    let account_ids = match service.record_cache_search_visibility(
        page,
        total_messages,
        result_count,
    ) {
        Ok(account_ids) => account_ids,
        Err(error) => {
            ph_warn!(events::CACHE_SEARCH_VISIBILITY_RECORD_FAILED, error = %error, "failed to record cache search visibility signals");
            return;
        }
    };
    trigger_cache_maintenance(supervisor, account_ids, request.operation_id).await;
}

fn counts(
    service: &Arc<MailService>,
    scope_rule: &SmartMailboxRule,
    result_rule: &SmartMailboxRule,
) -> Option<(u64, u64)> {
    let total_messages = match service.count_messages_by_rule(scope_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(events::CACHE_SEARCH_VISIBILITY_SCOPE_COUNT_FAILED, error = %error, "skipping cache search visibility signals because scope count failed");
            return None;
        }
    };
    let result_count = match service.count_messages_by_rule(result_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(events::CACHE_SEARCH_VISIBILITY_RESULT_COUNT_FAILED, error = %error, "skipping cache search visibility signals because result count failed");
            return None;
        }
    };
    Some((total_messages, result_count))
}

async fn trigger_cache_maintenance(
    supervisor: &Arc<AccountSupervisor>,
    account_ids: Vec<AccountId>,
    operation_id: Option<String>,
) {
    for account_id in account_ids {
        if let Err(error) = supervisor
            .trigger_cache_maintenance(&account_id, operation_id.clone())
            .await
        {
            ph_warn!(events::CACHE_MAINTENANCE_TRIGGER_FAILED, account_id = %account_id, error = %error, "failed to trigger cache maintenance after search visibility signal");
        }
    }
}
