use std::sync::Arc;

use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, validate_account_settings, validate_automation_drafts,
    validate_automation_rules, validate_default_account_exists, AccountAppearance, AccountDriver,
    AccountId, AccountOverview, AccountSettings, AccountTransportSettings, AppSettings,
    AutomationAction, AutomationRule, CachePolicy, DomainEvent, Id, ImapTransportSettings,
    MailService, MailStore, MailboxId, MessageSortField, ProviderAuthKind, ProviderHint,
    ServiceError, SmartMailbox, SmartMailboxId, SmartMailboxKind, SmtpTransportSettings,
    SortDirection, StoreError, EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_ACCOUNT_UPDATED, EVENT_TOPIC_CONFIG_RELOADED, EVENT_TOPIC_SETTINGS_UPDATED,
    EVENT_TOPIC_SMART_MAILBOX_CREATED, EVENT_TOPIC_SMART_MAILBOX_DELETED,
    EVENT_TOPIC_SMART_MAILBOX_RESET, EVENT_TOPIC_SMART_MAILBOX_UPDATED,
};
use posthaste_runtime_contract::{
    AccountTransportMutation, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation,
    PatchAccountMutation, PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeError,
    RuntimeErrorCode, SecretWriteMode, SecretWriteMutation,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::broadcast;

use crate::account_reads::AccountReadService;
use crate::account_repository::AccountRepository;
use crate::oauth::{OAuthExchangeResult, OAuthProviderProfile, OAuthTokenSet};
use crate::supervisor::AccountSupervisor;

mod accounts;
mod app_settings;
mod automation;
mod smart_mailboxes;

use automation::*;

const GLOBAL_EVENT_ACCOUNT_ID: &str = "app";

pub struct AccountMutationService {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    account_repository: Arc<AccountRepository>,
    event_sender: broadcast::Sender<DomainEvent>,
    supervisor: Arc<AccountSupervisor>,
    reads: Arc<AccountReadService>,
}

impl AccountMutationService {
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        account_repository: Arc<AccountRepository>,
        event_sender: broadcast::Sender<DomainEvent>,
        supervisor: Arc<AccountSupervisor>,
        reads: Arc<AccountReadService>,
    ) -> Self {
        Self {
            service,
            store,
            account_repository,
            event_sender,
            supervisor,
            reads,
        }
    }

    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    fn append_and_publish_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        payload: serde_json::Value,
    ) -> Result<(), RuntimeError> {
        let event = self
            .store
            .append_event(account_id, topic, None, None, payload)
            .map_err(store_error_to_runtime_error)?;
        self.publish_events(std::slice::from_ref(&event));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ResourceKind {
    Account,
    AppSettings,
    Config,
    SmartMailbox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ResourceOperation {
    Created,
    Updated,
    Deleted,
    Reloaded,
    Reset,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceChange {
    kind: ResourceKind,
    operation: ResourceOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}
impl ResourceChange {
    fn account(operation: ResourceOperation, account_id: &AccountId) -> Self {
        Self {
            kind: ResourceKind::Account,
            operation,
            id: Some(account_id.as_str().to_string()),
            account_id: Some(account_id.as_str().to_string()),
        }
    }
    fn config_reloaded() -> Self {
        Self {
            kind: ResourceKind::Config,
            operation: ResourceOperation::Reloaded,
            id: None,
            account_id: None,
        }
    }

    fn app_settings_updated() -> Self {
        Self {
            kind: ResourceKind::AppSettings,
            operation: ResourceOperation::Updated,
            id: None,
            account_id: None,
        }
    }

    fn smart_mailbox(operation: ResourceOperation, smart_mailbox_id: &SmartMailboxId) -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation,
            id: Some(smart_mailbox_id.as_str().to_string()),
            account_id: None,
        }
    }

    fn smart_mailbox_reset() -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation: ResourceOperation::Reset,
            id: None,
            account_id: None,
        }
    }
}

fn account_operation_from_topic(topic: &str) -> ResourceOperation {
    match topic {
        EVENT_TOPIC_ACCOUNT_CREATED => ResourceOperation::Created,
        EVENT_TOPIC_ACCOUNT_DELETED => ResourceOperation::Deleted,
        _ => ResourceOperation::Updated,
    }
}

fn account_event_payload(topic: &str, account_id: &AccountId) -> serde_json::Value {
    let operation = account_operation_from_topic(topic);
    json!({
        "accountId": account_id.as_str(),
        "resources": [ResourceChange::account(operation, account_id)],
    })
}

fn config_event_payload(
    resources: Vec<ResourceChange>,
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut payload = match extra {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    payload.insert("resources".to_string(), json!(resources));
    serde_json::Value::Object(payload)
}

pub(crate) fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
}
