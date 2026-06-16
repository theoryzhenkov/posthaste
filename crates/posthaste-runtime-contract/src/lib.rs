//! Transport-neutral runtime contract shared by Posthaste runtime implementations.
//!
//! The types in this crate intentionally avoid Axum, Tauri, frontend, provider-client,
//! SQLite-table, or replica-table dependencies.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#contract-no-transport-types

mod mail_query;

pub use mail_query::*;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use posthaste_domain::{
    AccountAppearance, AccountDriver, AccountId, AccountOverview, AddToMailboxCommand, AppSettings,
    AutomationRule, CachePolicy, CachedSenderAddress, CommandResult, DomainEvent, EventFilter,
    Identity, ImapTransportSettings, MailboxId, MailboxSummary, MessageId, MessageSummary,
    ProviderAuthKind, ProviderHint, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    ReplyContext, SendMessageRequest, ServiceError, ServiceErrorKind, SetKeywordsCommand,
    SmartMailbox, SmartMailboxId, SmartMailboxRule, SmartMailboxSummary, SmtpTransportSettings,
    SyncMode, TagSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

macro_rules! define_id {
    ($name:ident, u64, $getter:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> Self {
                Self(value)
            }

            pub fn $getter(self) -> u64 {
                self.0
            }
        }
    };
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(String);

            impl $name {
                pub fn new(value: impl Into<String>) -> Self {
                    Self(value.into())
                }

                pub fn as_str(&self) -> &str {
                    &self.0
                }
            }
        )+
    };
}

define_id!(
    RuntimeSessionId,
    ViewId,
    SubscriptionId,
    ClientMutationId,
    RuntimeMutationId,
);
define_id!(ViewRevision, u64, get);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaller {
    pub session_id: Option<RuntimeSessionId>,
    pub capabilities: RuntimeCallerCapabilities,
    pub account_scope: Option<Vec<String>>,
    pub operation_source: RuntimeOperationSource,
    pub correlation_id: Option<String>,
}

impl RuntimeCaller {
    pub fn system() -> Self {
        Self {
            session_id: None,
            capabilities: RuntimeCallerCapabilities::default(),
            account_scope: None,
            operation_source: RuntimeOperationSource::System,
            correlation_id: None,
        }
    }

    pub fn api() -> Self {
        Self {
            operation_source: RuntimeOperationSource::Api,
            ..Self::system()
        }
    }

    pub fn test() -> Self {
        Self {
            operation_source: RuntimeOperationSource::Test,
            ..Self::system()
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallerCapabilities {
    #[serde(default)]
    pub actions: Vec<RuntimeCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCapability {
    Read,
    Manage,
    Send,
    Tag,
    Move,
    Delete,
    Resource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeOperationSource {
    System,
    Api,
    Desktop,
    Renderer,
    Test,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountList {
    pub ids: Vec<AccountId>,
    pub enabled_ids: Vec<AccountId>,
    pub items: Vec<AccountOverview>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AccountScopeRequest {
    EnabledAccounts,
    Explicit { account_ids: Vec<AccountId> },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportMutation {
    pub provider: Option<ProviderHint>,
    pub auth: Option<ProviderAuthKind>,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretWriteMode {
    #[default]
    Keep,
    Replace,
    Clear,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteMutation {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountMutation {
    pub id: Option<String>,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    #[serde(default)]
    pub transport: AccountTransportMutation,
    #[serde(default)]
    pub secret: SecretWriteMutation,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAccountMutation {
    pub name: Option<String>,
    pub full_name: Option<String>,
    pub email_patterns: Option<Vec<String>>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    pub transport: Option<AccountTransportMutation>,
    pub secret: Option<SecretWriteMutation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAppSettingsMutation {
    #[serde(default)]
    pub default_account_id: Option<Option<String>>,
    pub cache_policy: Option<CachePolicy>,
    pub automation_rules: Option<Vec<AutomationRule>>,
    pub automation_drafts: Option<Vec<AutomationRule>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewMutation {
    pub condition: SmartMailboxRule,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewResult {
    pub total: i64,
    pub items: Vec<MessageSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmartMailboxMutation {
    pub name: String,
    pub position: Option<i64>,
    pub rule: SmartMailboxRule,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSmartMailboxMutation {
    pub name: Option<String>,
    pub position: Option<i64>,
    pub rule: Option<SmartMailboxRule>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountVerificationResult {
    pub ok: bool,
    pub identity_email: Option<String>,
    pub push_supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub lifecycle: RuntimeLifecycle,
    pub store: RuntimeStoreStatus,
    pub account_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAttachmentBytes {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub filename: Option<String>,
}

/// Live runtime event stream returned by authority runtimes.
pub type RuntimeEventStream = BoxStream<'static, DomainEvent>;

/// Runtime-owned event subscription: optional replayed backlog followed by live events.
pub struct RuntimeEventSubscription {
    pub replay: Vec<DomainEvent>,
    pub live: RuntimeEventStream,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLifecycle {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStoreStatus {
    pub config_loaded: bool,
    pub state_store_open: bool,
    pub cache_root_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDescriptor {
    pub family: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewLifecycle {
    Loading,
    Ready,
    Updating,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadWatermark {
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoverage {
    pub kind: RuntimeCoverageKind,
    #[serde(default)]
    pub details: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCoverageKind {
    Complete,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSnapshot {
    pub view_id: ViewId,
    pub descriptor: ViewDescriptor,
    pub revision: ViewRevision,
    pub lifecycle: ViewLifecycle,
    pub read_watermark: Option<ReadWatermark>,
    pub coverage: RuntimeCoverage,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub pending_mutations: Vec<RuntimeMutationId>,
    pub error: Option<RuntimeAdapterError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationRequest {
    pub session_id: Option<RuntimeSessionId>,
    pub name: String,
    #[serde(default)]
    pub args: Value,
    pub client_mutation_id: ClientMutationId,
    pub context: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    pub runtime_mutation_id: Option<RuntimeMutationId>,
    pub client_mutation_id: ClientMutationId,
    pub state: MutationSettlementState,
    pub error: Option<RuntimeAdapterError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MutationSettlementState {
    Accepted,
    LocalApplied,
    Queued,
    Confirmed,
    Failed,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAdapterError {
    pub code: RuntimeErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCode {
    RuntimeNotReady,
    InvalidDescriptor,
    InvalidMutation,
    InvalidSecret,
    InvalidAccount,
    AccountBaseUrlRequired,
    AccountSecretRequired,
    AccountUsernameRequired,
    AccountSenderRequired,
    Unauthorized,
    NotFound,
    ProviderUnavailable,
    Conflict,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    StorageFailure,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
    TransportDisconnected,
    Internal,
}

#[derive(Debug)]
pub struct RuntimeError(pub RuntimeAdapterError);

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn new(code: RuntimeErrorCode, message: impl Into<String>) -> Self {
        Self::with_details(code, message, Value::Null)
    }

    pub fn with_details(
        code: RuntimeErrorCode,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self(RuntimeAdapterError {
            code,
            message: message.into(),
            retryable: false,
            correlation_id: None,
            details,
        })
    }

    pub fn retryable(code: RuntimeErrorCode, message: impl Into<String>) -> Self {
        Self(RuntimeAdapterError {
            code,
            message: message.into(),
            retryable: true,
            correlation_id: None,
            details: Value::Null,
        })
    }

    pub fn internal(message: impl Into<String>, correlation_id: Option<String>) -> Self {
        Self(RuntimeAdapterError {
            code: RuntimeErrorCode::Internal,
            message: message.into(),
            retryable: false,
            correlation_id,
            details: Value::Null,
        })
    }

    pub fn runtime_not_ready(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::RuntimeNotReady, message)
    }

    pub fn invalid_descriptor(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidDescriptor, message)
    }

    pub fn invalid_mutation(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidMutation, message)
    }

    pub fn invalid_secret(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidSecret, message)
    }

    pub fn invalid_account(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidAccount, message)
    }

    pub fn account_base_url_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountBaseUrlRequired, message)
    }

    pub fn account_secret_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountSecretRequired, message)
    }

    pub fn account_username_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountUsernameRequired, message)
    }

    pub fn account_sender_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountSenderRequired, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::Unauthorized, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::NotFound, message)
    }

    pub fn provider_unavailable(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::ProviderUnavailable, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::Conflict, message)
    }

    pub fn envelope(&self) -> &RuntimeAdapterError {
        &self.0
    }
}

impl From<ServiceError> for RuntimeError {
    fn from(error: ServiceError) -> Self {
        let code = match error.kind() {
            ServiceErrorKind::NotFound => RuntimeErrorCode::NotFound,
            ServiceErrorKind::Conflict => RuntimeErrorCode::Conflict,
            ServiceErrorKind::StateMismatch => RuntimeErrorCode::StateMismatch,
            ServiceErrorKind::AuthError => RuntimeErrorCode::Unauthorized,
            ServiceErrorKind::GatewayUnavailable => RuntimeErrorCode::ProviderUnavailable,
            ServiceErrorKind::NetworkError => RuntimeErrorCode::NetworkError,
            ServiceErrorKind::CannotCalculateChanges => RuntimeErrorCode::CannotCalculateChanges,
            ServiceErrorKind::GatewayRejected => RuntimeErrorCode::GatewayRejected,
            ServiceErrorKind::SecretUnavailable => RuntimeErrorCode::SecretUnavailable,
            ServiceErrorKind::SecretUnsupported => RuntimeErrorCode::SecretUnsupported,
            ServiceErrorKind::StorageFailure => RuntimeErrorCode::StorageFailure,
            ServiceErrorKind::ConfigValidation => RuntimeErrorCode::ConfigValidation,
            ServiceErrorKind::ConfigIo => RuntimeErrorCode::ConfigIo,
            ServiceErrorKind::ConfigParse => RuntimeErrorCode::ConfigParse,
        };
        Self::new(code, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain::StoreError;

    #[test]
    fn service_error_conversion_preserves_runtime_error_code() {
        let error = RuntimeError::from(ServiceError::from(StoreError::NotFound(
            "account missing".to_string(),
        )));

        assert_eq!(error.envelope().code, RuntimeErrorCode::NotFound);
    }

    #[test]
    fn retryable_constructor_marks_retryable_envelope() {
        let error =
            RuntimeError::retryable(RuntimeErrorCode::ProviderUnavailable, "gateway unavailable");

        assert!(error.envelope().retryable);
        assert_eq!(error.envelope().code, RuntimeErrorCode::ProviderUnavailable);
    }
}

#[async_trait]
pub trait RuntimeCore: Send + Sync {
    async fn runtime_status(&self, caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError>;

    async fn get_app_settings(&self, caller: RuntimeCaller) -> Result<AppSettings, RuntimeError>;

    async fn patch_app_settings(
        &self,
        caller: RuntimeCaller,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError>;

    async fn preview_automation_rule(
        &self,
        caller: RuntimeCaller,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError>;

    async fn list_accounts(
        &self,
        caller: RuntimeCaller,
    ) -> Result<RuntimeAccountList, RuntimeError>;

    async fn get_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn resolve_account_scope(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError>;

    async fn list_mailboxes(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError>;

    async fn list_smart_mailboxes(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError>;

    async fn get_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn create_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn patch_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn delete_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError>;

    async fn reset_default_smart_mailboxes(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError>;

    async fn list_tags(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError>;

    async fn get_identity(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError>;

    async fn list_sender_addresses(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError>;

    async fn get_reply_context(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError>;

    async fn query_mail_page(
        &self,
        caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError>;

    async fn send_message(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError>;

    async fn set_message_keywords(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandResult, RuntimeError>;

    async fn add_message_to_mailbox(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandResult, RuntimeError>;

    async fn remove_message_from_mailbox(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandResult, RuntimeError>;

    async fn replace_message_mailboxes(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandResult, RuntimeError>;

    async fn destroy_message(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandResult, RuntimeError>;

    async fn set_mailbox_role(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError>;

    async fn get_message_detail(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandResult, RuntimeError>;

    async fn get_message_attachment(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        attachment_id: String,
    ) -> Result<RuntimeAttachmentBytes, RuntimeError>;

    async fn sync_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError>;

    async fn replay_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError>;

    async fn subscribe_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError>;

    async fn create_account(
        &self,
        caller: RuntimeCaller,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn patch_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn delete_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError>;

    async fn verify_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError>;

    async fn set_account_enabled(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError>;

    async fn reload_config(&self, caller: RuntimeCaller) -> Result<(), RuntimeError>;
}
