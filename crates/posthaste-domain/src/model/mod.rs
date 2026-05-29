use std::fmt::{Display, Formatter};
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::{
    cache::{CacheFetchUnit, CachePolicy},
    imap::{ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationKey},
    ConfigError, ProviderKind,
};

mod appearance;
pub use appearance::*;

/// Generates a newtype wrapper around `String` for type-safe identifiers.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id!(
    /// Opaque server-assigned identifier for a mail account.
    ///
    /// @spec docs/L0-accounts#the-invariant
    AccountId
);

string_id!(
    /// Opaque server-assigned identifier for a mailbox (folder or label).
    ///
    /// @spec docs/L1-jmap#core-types
    MailboxId
);

string_id!(
    /// Opaque server-assigned identifier for a single email message.
    ///
    /// @spec docs/L1-jmap#core-types
    MessageId
);

string_id!(
    /// Opaque server-assigned identifier for a JMAP thread.
    ///
    /// @spec docs/L1-jmap#core-types
    ThreadId
);

string_id!(
    /// Opaque server-assigned identifier for a binary blob (attachment or body content).
    ///
    /// @spec docs/L1-jmap#methods-used
    BlobId
);

string_id!(
    /// Locally-derived identifier for a conversation (cross-source thread grouping).
    ///
    /// @spec docs/L1-sync#conversation-pagination
    ConversationId
);

string_id!(
    /// Identifier for a smart mailbox (saved query with display metadata).
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    SmartMailboxId
);

/// Default timestamp for missing `created_at`/`updated_at` fields in config.
///
/// @spec docs/L1-accounts#toml-schema
pub const RFC3339_EPOCH: &str = "1970-01-01T00:00:00Z";

/// Event topic emitted after a successful sync cycle completes.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_SYNC_COMPLETED: &str = "sync.completed";

/// Event topic emitted when a sync cycle fails.
///
/// @spec docs/L1-sync#error-handling
pub const EVENT_TOPIC_SYNC_FAILED: &str = "sync.failed";

/// Event topic emitted when application settings change.
///
/// @spec docs/L1-api#settings
pub const EVENT_TOPIC_SETTINGS_UPDATED: &str = "settings.updated";

/// Event topic emitted after an external config reload.
///
/// @spec docs/L1-api#sync-and-events
pub const EVENT_TOPIC_CONFIG_RELOADED: &str = "config.reloaded";

/// Event topic emitted when a smart mailbox is created.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_CREATED: &str = "smart_mailbox.created";

/// Event topic emitted when a smart mailbox is updated.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_UPDATED: &str = "smart_mailbox.updated";

/// Event topic emitted when a smart mailbox is deleted.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_DELETED: &str = "smart_mailbox.deleted";

/// Event topic emitted when default smart mailboxes are reset.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_RESET: &str = "smart_mailbox.reset";

/// Event topic emitted when message metadata changes (keywords, mailboxes).
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_UPDATED: &str = "message.updated";

/// Event topic emitted when message keywords change.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED: &str = "message.keywords_changed";

/// Event topic emitted when a message body is cached locally.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_BODY_CACHED: &str = "message.body_cached";

/// Event topic emitted when message mailbox membership changes.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED: &str = "message.mailboxes_changed";

/// Event topic emitted when a new message arrives in a mailbox.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_ARRIVED: &str = "message.arrived";

/// Event topic emitted when a mailbox is created, updated, or deleted.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MAILBOX_UPDATED: &str = "mailbox.updated";

/// Event topic emitted when account configuration changes.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_UPDATED: &str = "account.updated";

/// Event topic emitted when a new account is created.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_CREATED: &str = "account.created";

/// Event topic emitted when an account is deleted.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_DELETED: &str = "account.deleted";

/// Event topic emitted when account runtime status transitions.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_STATUS_CHANGED: &str = "account.status_changed";

/// Event topic emitted when a push transport connects successfully.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_CONNECTED: &str = "push.connected";

/// Event topic emitted when a push transport disconnects or fails.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_DISCONNECTED: &str = "push.disconnected";

/// Every event topic the server emits, in declaration order.
///
/// Single source of truth for the documented topic set: the committed
/// `asyncapi.json` event contract is drift-checked against this slice.
///
/// @spec docs/L1-api#sse-event-stream
pub const ALL_EVENT_TOPICS: &[&str] = &[
    EVENT_TOPIC_SYNC_COMPLETED,
    EVENT_TOPIC_SYNC_FAILED,
    EVENT_TOPIC_SETTINGS_UPDATED,
    EVENT_TOPIC_CONFIG_RELOADED,
    EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED,
    EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_MESSAGE_UPDATED,
    EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED,
    EVENT_TOPIC_MESSAGE_BODY_CACHED,
    EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
    EVENT_TOPIC_MESSAGE_ARRIVED,
    EVENT_TOPIC_MAILBOX_UPDATED,
    EVENT_TOPIC_ACCOUNT_UPDATED,
    EVENT_TOPIC_ACCOUNT_CREATED,
    EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
    EVENT_TOPIC_PUSH_CONNECTED,
    EVENT_TOPIC_PUSH_DISCONNECTED,
];

/// Global application settings shared across all accounts.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppSettings {
    pub default_account_id: Option<AccountId>,
    #[serde(default)]
    pub appearance: AppAppearanceSettings,
    #[serde(default)]
    pub cache_policy: CachePolicy,
    #[serde(default)]
    pub automation_rules: Vec<AutomationRule>,
    #[serde(default)]
    pub automation_drafts: Vec<AutomationRule>,
}

/// Backend driver type for an account.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountDriver {
    Jmap,
    ImapSmtp,
    Mock,
}

/// Provider-level capabilities that affect domain planning.
///
/// @spec docs/L1-sync#local-cache-planning
/// @spec docs/L1-jmap#push
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountDriverCapabilities {
    pub cache_fetch_unit: CacheFetchUnit,
    pub supports_push: bool,
}

impl AccountDriver {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::ImapSmtp => "imap_smtp",
            Self::Mock => "mock",
        }
    }

    pub fn capabilities(&self) -> AccountDriverCapabilities {
        match self {
            Self::Jmap => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::BodyOnly,
                supports_push: true,
            },
            Self::ImapSmtp => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::RawMessage,
                supports_push: false,
            },
            Self::Mock => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::BodyOnly,
                supports_push: false,
            },
        }
    }
}

/// Storage backend for account credentials.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SecretKind {
    /// Credential read from an environment variable.
    Env,
    /// Credential stored in the OS keyring (macOS Keychain).
    Os,
}

/// Pointer to a stored secret, combining storage kind and lookup key.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SecretRef {
    pub kind: SecretKind,
    pub key: String,
}

/// Alias for [`SecretKind`], used in API responses to describe where a secret is stored.
///
/// @spec docs/L1-api#secret-management
pub type SecretStorage = SecretKind;

/// Redacted secret status returned in API responses. Never contains the secret value.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SecretStatus {
    pub storage: SecretStorage,
    pub configured: bool,
    pub label: Option<String>,
}

/// User-selected provider hint for traditional mail account setup.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProviderHint {
    #[default]
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

impl From<ProviderKind> for ProviderHint {
    fn from(provider: ProviderKind) -> Self {
        match provider {
            ProviderKind::Generic => Self::Generic,
            ProviderKind::Gmail => Self::Gmail,
            ProviderKind::Outlook => Self::Outlook,
            ProviderKind::Icloud => Self::Icloud,
        }
    }
}

/// Authentication mode used by the selected provider transport.
///
/// @spec docs/L0-providers#authentication
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProviderAuthKind {
    #[default]
    Password,
    AppPassword,
    #[serde(rename = "oauth2")]
    OAuth2,
}

/// TLS behavior for IMAP and SMTP endpoints.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum TransportSecurity {
    #[default]
    Tls,
    StartTls,
    Plain,
}

/// IMAP endpoint settings for traditional provider sync.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ImapTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// SMTP endpoint settings for traditional provider submission.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmtpTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// Transport-layer settings for connecting to a provider.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountTransportSettings {
    #[serde(default)]
    pub provider: ProviderHint,
    #[serde(default)]
    pub auth: ProviderAuthKind,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRef>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

impl AccountTransportSettings {
    pub fn provider_kind(&self) -> ProviderKind {
        ProviderKind::from(&self.provider)
    }
}

/// User-facing visual identity for an account.
///
/// @spec docs/L1-accounts#toml-schema
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountAppearance {
    Initials {
        initials: String,
        #[cfg_attr(feature = "openapi", schema(rename = "colorHue"))]
        color_hue: u16,
    },
    Image {
        #[cfg_attr(feature = "openapi", schema(rename = "imageId"))]
        image_id: String,
        initials: String,
        #[cfg_attr(feature = "openapi", schema(rename = "colorHue"))]
        color_hue: u16,
    },
}

/// Account-level automation rule evaluated by backend triggers.
///
/// @spec docs/L1-accounts#toml-schema
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub triggers: Vec<AutomationTrigger>,
    pub condition: SmartMailboxRule,
    pub actions: Vec<AutomationAction>,
    pub backfill: bool,
}

/// Durable state for backend-owned automation backfill work.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationBackfillJob {
    pub account_id: AccountId,
    pub rule_fingerprint: String,
    pub status: AutomationBackfillJobStatus,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub updated_at: String,
}

/// Lifecycle state for an automation backfill job.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationBackfillJobStatus {
    Pending,
    Completed,
}

/// Result of one durable automation backfill worker batch.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug)]
pub struct AutomationBackfillBatchOutcome {
    pub ran: bool,
    pub events: Vec<DomainEvent>,
    pub has_more: bool,
}

/// Result of one optional-content cache worker batch.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Default)]
pub struct CacheWorkerBatchOutcome {
    pub scanned: usize,
    pub attempted: usize,
    pub attempted_bytes: u64,
    pub cached: usize,
    pub cached_bytes: u64,
    pub failed: usize,
    pub skipped: usize,
    pub events: Vec<DomainEvent>,
}

/// Result of one optional-content cache re-score batch.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Default)]
pub struct CacheRescoreBatchOutcome {
    pub scanned: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Event types that can cause an automation rule to run.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AutomationTrigger {
    MessageArrived,
    MessageChanged,
    Manual,
}

/// Supported effects for automation rules.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AutomationAction {
    ApplyTag {
        tag: String,
    },
    RemoveTag {
        tag: String,
    },
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    MoveToMailbox {
        #[cfg_attr(feature = "openapi", schema(rename = "mailboxId"))]
        mailbox_id: MailboxId,
    },
}

/// Full persisted configuration for a mail account.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettings {
    pub id: AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub email_patterns: Vec<String>,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub appearance: Option<AccountAppearance>,
    pub transport: AccountTransportSettings,
    pub created_at: String,
    pub updated_at: String,
}

/// API-facing account connection variant used by account settings UIs.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountConnectionOverview {
    ManualCredentials {
        provider: ProviderHint,
        #[cfg_attr(feature = "openapi", schema(rename = "providerKind"))]
        provider_kind: ProviderKind,
        auth: ProviderAuthKind,
        #[cfg_attr(feature = "openapi", schema(rename = "baseUrl"))]
        base_url: Option<String>,
        username: Option<String>,
        imap: Option<ImapTransportSettings>,
        smtp: Option<SmtpTransportSettings>,
        secret: SecretStatus,
    },
    ManagedOAuth {
        provider: ProviderHint,
        #[cfg_attr(feature = "openapi", schema(rename = "providerKind"))]
        provider_kind: ProviderKind,
        auth: ProviderAuthKind,
        username: Option<String>,
        imap: Option<ImapTransportSettings>,
        smtp: Option<SmtpTransportSettings>,
        secret: SecretStatus,
    },
}

#[derive(Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
enum AccountConnectionOverviewCompat {
    ManualCredentials {
        provider: ProviderHint,
        provider_kind: Option<ProviderKind>,
        auth: ProviderAuthKind,
        base_url: Option<String>,
        username: Option<String>,
        imap: Option<ImapTransportSettings>,
        smtp: Option<SmtpTransportSettings>,
        secret: SecretStatus,
    },
    ManagedOAuth {
        provider: ProviderHint,
        provider_kind: Option<ProviderKind>,
        auth: ProviderAuthKind,
        username: Option<String>,
        imap: Option<ImapTransportSettings>,
        smtp: Option<SmtpTransportSettings>,
        secret: SecretStatus,
    },
}

impl<'de> Deserialize<'de> for AccountConnectionOverview {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match AccountConnectionOverviewCompat::deserialize(deserializer)? {
                AccountConnectionOverviewCompat::ManualCredentials {
                    provider,
                    provider_kind,
                    auth,
                    base_url,
                    username,
                    imap,
                    smtp,
                    secret,
                } => AccountConnectionOverview::ManualCredentials {
                    provider_kind: provider_kind.unwrap_or_else(|| ProviderKind::from(&provider)),
                    provider,
                    auth,
                    base_url,
                    username,
                    imap,
                    smtp,
                    secret,
                },
                AccountConnectionOverviewCompat::ManagedOAuth {
                    provider,
                    provider_kind,
                    auth,
                    username,
                    imap,
                    smtp,
                    secret,
                } => AccountConnectionOverview::ManagedOAuth {
                    provider_kind: provider_kind.unwrap_or_else(|| ProviderKind::from(&provider)),
                    provider,
                    auth,
                    username,
                    imap,
                    smtp,
                    secret,
                },
            },
        )
    }
}

/// Runtime health status of a mail account.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountStatus {
    Ready,
    Syncing,
    Degraded,
    AuthError,
    Offline,
    Disabled,
}

/// Current state of the push notification transport for an account.
///
/// @spec docs/L2-transport#push-transport
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PushStatus {
    Connected,
    Reconnecting,
    Unsupported,
    Disabled,
}

/// Coarse user-facing phase for a running sync cycle.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SyncProgressStage {
    Connecting,
    Discovering,
    Planning,
    Fetching,
    Storing,
    Waiting,
}

/// Current user-facing progress for an account sync.
///
/// This is intentionally compact and coarse. Provider adapters may report more
/// frequent internal logs, but the runtime overview only exposes stable status
/// useful to users.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SyncProgress {
    pub sync_id: String,
    pub trigger: SyncTrigger,
    pub started_at: String,
    pub stage: SyncProgressStage,
    pub detail: String,
    pub mailbox_name: Option<String>,
    pub mailbox_index: Option<usize>,
    pub mailbox_count: Option<usize>,
    pub message_count: Option<usize>,
    pub total_count: Option<usize>,
}

/// Volatile runtime state for an account (sync status, push status, last error).
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountRuntimeOverview {
    pub status: AccountStatus,
    pub push: PushStatus,
    pub last_sync_at: Option<String>,
    pub last_sync_error: Option<String>,
    pub last_sync_error_code: Option<String>,
    pub sync_progress: Option<SyncProgress>,
}

impl Default for AccountRuntimeOverview {
    fn default() -> Self {
        Self {
            status: AccountStatus::Offline,
            push: PushStatus::Disabled,
            last_sync_at: None,
            last_sync_error: None,
            last_sync_error_code: None,
            sync_progress: None,
        }
    }
}

/// Combined account config and runtime state returned by the API.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountOverview {
    pub id: AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub email_patterns: Vec<String>,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub appearance: AccountAppearance,
    pub connection: AccountConnectionOverview,
    pub created_at: String,
    pub updated_at: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub runtime: AccountRuntimeOverview,
}

/// Per-type, per-account JMAP state string used for delta sync.
///
/// @spec docs/L1-sync#state-management
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursor {
    pub object_type: SyncObject,
    pub state: String,
    pub updated_at: String,
}

impl SyncCursor {
    /// Return the provider state token stored in this cursor.
    ///
    /// Most cursors store the provider token directly. Some JMAP email cursors
    /// wrap the provider token with local metadata versioning so Posthaste can
    /// force a full metadata refresh when its projection changes.
    pub fn provider_state(&self) -> String {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.state) else {
            return self.state.clone();
        };
        if value.get("kind").and_then(serde_json::Value::as_str) != Some("jmap-email") {
            return self.state.clone();
        }
        value
            .get("state")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| self.state.clone())
    }
}

/// JMAP object type that participates in delta sync.
///
/// @spec docs/L1-sync#state-management
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncObject {
    Mailbox,
    Message,
}

impl SyncObject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Message => "message",
        }
    }
}

/// User-requested sync mode.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SyncMode {
    #[default]
    Incremental,
    FullMetadata,
}

impl SyncMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::FullMetadata => "fullMetadata",
        }
    }

    pub fn requires_full_message_metadata(self) -> bool {
        matches!(self, Self::FullMetadata)
    }
}

/// Metadata for a locally-cached raw MIME message file.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RawMessageRef {
    pub path: String,
    pub sha256: String,
    pub size: i64,
    pub mime_type: String,
    pub fetched_at: String,
}

/// Lightweight mailbox view for sidebar and list endpoints.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MailboxSummary {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

/// Message metadata for list views (no body content).
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageSummary {
    pub id: MessageId,
    pub source_id: AccountId,
    pub source_name: String,
    pub source_thread_id: ThreadId,
    pub conversation_id: ConversationId,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub to: Vec<Recipient>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
}

/// Column by which message lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MessageSortField {
    #[default]
    Date,
    From,
    Subject,
    Source,
    Flagged,
    Attachment,
}

/// Opaque seek-pagination cursor for message lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCursor {
    pub sort_value: String,
    pub source_id: AccountId,
    pub message_id: MessageId,
}

/// A single page of message summaries with an optional cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<MessageCursor>,
}

/// Full message including sanitized body content, returned by message detail endpoint.
///
/// @spec docs/L1-api#message-body-sanitization
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageAttachment {
    pub id: String,
    pub blob_id: BlobId,
    pub part_id: Option<String>,
    pub filename: Option<String>,
    pub mime_type: String,
    pub size: i64,
    pub disposition: Option<String>,
    pub cid: Option<String>,
    pub is_inline: bool,
}

/// Full message including sanitized body content, returned by message detail endpoint.
///
/// @spec docs/L1-api#message-body-sanitization
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_message: Option<RawMessageRef>,
    pub attachments: Vec<MessageAttachment>,
}

/// All messages belonging to a single JMAP thread, ordered by `receivedAt`.
///
/// @spec docs/L1-search#thread-view
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub id: ThreadId,
    pub messages: Vec<MessageSummary>,
}

/// Account-qualified reference to a specific message.
///
/// @spec docs/L0-accounts#the-invariant
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SourceMessageRef {
    pub source_id: AccountId,
    pub message_id: MessageId,
}

/// Conversation row for the paginated middle pane.
///
/// @spec docs/L1-sync#conversation-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub preview: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub latest_received_at: String,
    pub unread_count: i64,
    pub message_count: i64,
    pub source_ids: Vec<AccountId>,
    pub source_names: Vec<String>,
    pub latest_message: SourceMessageRef,
    pub latest_source_name: String,
    pub has_attachment: bool,
    pub is_flagged: bool,
}

/// Column by which conversation lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConversationSortField {
    #[default]
    Date,
    From,
    Subject,
    Source,
    ThreadSize,
    Flagged,
    Attachment,
}

/// Sort direction for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

/// Opaque seek-pagination cursor for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCursor {
    pub sort_value: String,
    pub conversation_id: ConversationId,
}

/// A single page of conversation summaries with an optional cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<ConversationCursor>,
}

/// Full conversation detail with all messages expanded.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationView {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub messages: Vec<MessageSummary>,
}

/// An account with its mailboxes, as rendered in the sidebar.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SidebarSource {
    pub id: AccountId,
    pub name: String,
    pub mailboxes: Vec<MailboxSummary>,
}

/// Distinguishes built-in smart mailboxes from user-created ones.
///
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxKind {
    Default,
    User,
}

/// Smart mailbox entry with live counts for the sidebar.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SidebarSmartMailbox {
    pub id: SmartMailboxId,
    pub name: String,
    pub unread_messages: i64,
    pub total_messages: i64,
}

/// User-facing tag derived from non-system JMAP keywords.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TagSummary {
    pub name: String,
    pub unread_messages: i64,
    pub total_messages: i64,
}

/// Combined sidebar payload: smart mailboxes at the top, then per-source mailboxes.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SidebarResponse {
    pub smart_mailboxes: Vec<SidebarSmartMailbox>,
    pub tags: Vec<TagSummary>,
    pub sources: Vec<SidebarSource>,
}

/// Boolean combinator for smart mailbox rule groups: `All` (AND) or `Any` (OR).
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxGroupOperator {
    All,
    Any,
}

/// Message field that a smart mailbox condition can filter on.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxField {
    SourceId,
    SourceName,
    MessageId,
    ThreadId,
    MailboxId,
    MailboxName,
    MailboxRole,
    IsRead,
    IsFlagged,
    HasAttachment,
    Keyword,
    FromName,
    FromEmail,
    Subject,
    Preview,
    ReceivedAt,
}

/// Comparison operator for a smart mailbox condition.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxOperator {
    Equals,
    In,
    Contains,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
}

/// Condition value: scalar string, string list (for `In`), or boolean.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxValue {
    String(String),
    Strings(Vec<String>),
    Bool(bool),
}

/// Boolean group node containing child conditions or nested groups.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxGroup {
    pub operator: SmartMailboxGroupOperator,
    pub negated: bool,
    // Break the SmartMailboxGroup -> SmartMailboxRuleNode -> SmartMailboxGroup
    // schema cycle so utoipa's component collector does not recurse infinitely.
    // The emitted schema still references SmartMailboxRuleNode by `$ref`.
    #[cfg_attr(feature = "openapi", schema(no_recursion))]
    pub nodes: Vec<SmartMailboxRuleNode>,
}

/// Leaf condition matching a single field with an operator and value.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxCondition {
    pub field: SmartMailboxField,
    pub operator: SmartMailboxOperator,
    pub negated: bool,
    pub value: SmartMailboxValue,
}

/// Recursive rule tree node: either a [`SmartMailboxGroup`] or a [`SmartMailboxCondition`].
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxRuleNode {
    Group(SmartMailboxGroup),
    Condition(SmartMailboxCondition),
}

/// Top-level rule for a smart mailbox, wrapping a root group.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxRule {
    pub root: SmartMailboxGroup,
}

/// A saved query with display metadata that behaves like a virtual mailbox.
///
/// @spec docs/L0-search#smart-mailboxes
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailbox {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    /// Identifies built-in smart mailboxes (e.g. "inbox", "trash").
    pub default_key: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub rule: SmartMailboxRule,
    pub created_at: String,
    pub updated_at: String,
}

/// Smart mailbox config with live unread/total counts from the store.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxSummary {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    pub default_key: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub unread_messages: i64,
    pub total_messages: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Mailbox state from a JMAP sync response, used in [`SyncBatch`].
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxRecord {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

/// Full email record from a JMAP sync response, used in [`SyncBatch`].
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    pub id: MessageId,
    pub source_thread_id: ThreadId,
    pub remote_blob_id: Option<BlobId>,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    #[serde(default)]
    pub to: Vec<Recipient>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub size: i64,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_mime: Option<String>,
    /// RFC 2822 `Message-ID` header, used for threading.
    pub rfc_message_id: Option<String>,
    /// RFC 2822 `In-Reply-To` header, used for threading.
    pub in_reply_to: Option<String>,
    /// RFC 2822 `References` header chain, used for threading.
    pub references: Vec<String>,
}

/// Builds a minimal RFC 2822 message from constituent parts for draft storage.
///
/// @spec docs/L1-compose#mime-structures
pub fn synthesize_plain_text_raw_mime(
    from_header: &str,
    subject: &str,
    body_text: Option<&str>,
) -> String {
    format!(
        "From: {from_header}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        body_text.unwrap_or("")
    )
}

/// Returns the current UTC time formatted as an RFC 3339 string.
pub fn now_iso8601() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())
}

/// Atomic unit of sync data applied within a single SQLite transaction.
///
/// When a `replace_all_*` flag is true, the store treats that object list as a
/// full snapshot and prunes any local objects not present in the batch.
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatch {
    pub mailboxes: Vec<MailboxRecord>,
    pub messages: Vec<MessageRecord>,
    pub imap_mailbox_states: Vec<ImapMailboxSyncState>,
    pub imap_message_locations: Vec<ImapMessageLocation>,
    /// IMAP location keys that disappeared from a mailbox-scoped delta.
    ///
    /// This is distinct from `deleted_message_ids`: one vanished IMAP UID can
    /// mean a Gmail label was removed while the canonical message still exists
    /// in another mailbox location.
    pub deleted_imap_message_locations: Vec<ImapMessageLocationKey>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    pub deleted_message_ids: Vec<MessageId>,
    /// When true, mailboxes are a full snapshot (from full resync fallback).
    pub replace_all_mailboxes: bool,
    /// When true, messages are a full snapshot (from full resync fallback).
    pub replace_all_messages: bool,
    pub cursors: Vec<SyncCursor>,
}

/// Lazily-fetched message body content returned by the gateway.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedBody {
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_mime: Option<String>,
    pub attachments: Vec<MessageAttachment>,
}

/// An ordered domain event stored in `event_log` and published via SSE.
///
/// @spec docs/L1-sync#event-propagation
/// @spec docs/L1-api#sse-event-stream
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DomainEvent {
    pub seq: i64,
    pub account_id: AccountId,
    pub topic: String,
    pub occurred_at: String,
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub payload: Value,
}

/// Query parameters for filtering the event log, used by `GET /v1/events`.
///
/// @spec docs/L1-api#sse-event-stream
#[derive(Clone, Debug)]
pub struct EventFilter {
    pub account_id: Option<AccountId>,
    pub topic: Option<String>,
    pub mailbox_id: Option<MailboxId>,
    pub after_seq: Option<i64>,
}

/// What caused a sync cycle to run.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SyncTrigger {
    Startup,
    Poll,
    Push,
    Manual,
}

impl SyncTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Poll => "poll",
            Self::Push => "push",
            Self::Manual => "manual",
        }
    }
}

/// A JMAP `StateChange` notification delivered over WebSocket or SSE.
///
/// @spec docs/L1-jmap#push
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotification {
    pub account_id: AccountId,
    pub changed: Vec<String>,
    pub received_at: String,
    /// Last-event-ID or push state for reconnection catch-up.
    pub checkpoint: Option<String>,
}

/// Async stream of push notifications from a single transport connection.
///
/// @spec docs/L1-jmap#push
pub type PushStream = Pin<Box<dyn Stream<Item = Result<PushNotification, GatewayError>> + Send>>;

/// Command to add and/or remove JMAP keywords on a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Command to atomically replace all mailbox memberships for a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Command to add a message to a single additional mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddToMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Command to remove a message from a single mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoveFromMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Result of a message mutation: updated detail (if applicable) and emitted events.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandResult {
    pub detail: Option<MessageDetail>,
    pub events: Vec<DomainEvent>,
}

/// Server-side outcome of a gateway mutation, carrying an updated sync cursor.
///
/// @spec docs/L1-sync#conflict-model
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationOutcome {
    pub cursor: Option<SyncCursor>,
}

/// JMAP sender identity for an account.
///
/// @spec docs/L1-jmap#core-types
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Email address with optional display name.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Locally cached sender address that previously passed provider submission.
///
/// @spec docs/L1-compose#sender-selection
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CachedSenderAddress {
    pub source_id: AccountId,
    pub name: Option<String>,
    pub email: String,
    pub last_used_at: String,
}

/// Pre-computed reply/forward metadata fetched from the gateway.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplyContext {
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// Request payload for sending a new email via `EmailSubmission/set`.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendMessageRequest {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// Errors from JMAP gateway operations.
///
/// @spec docs/L1-jmap#error-model
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway unavailable for account {0}")]
    Unavailable(String),
    #[error("authentication failed")]
    Auth,
    #[error("network error: {0}")]
    Network(String),
    #[error("state mismatch")]
    StateMismatch,
    #[error("cannot calculate changes")]
    CannotCalculateChanges,
    #[error("gateway rejected the request: {0}")]
    Rejected(String),
}

/// Errors from the local SQLite store.
///
/// @spec docs/L1-sync#error-handling
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage failure: {0}")]
    Failure(String),
}

/// Unified error type surfaced by [`crate::MailService`] and mapped to HTTP status codes.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
}

/// Stable service error category for exhaustive API status mapping.
///
/// @spec docs/L1-api#error-code-mapping
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceErrorKind {
    GatewayUnavailable,
    AuthError,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    NotFound,
    Conflict,
    StorageFailure,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
}

impl ServiceErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::GatewayUnavailable => "gateway_unavailable",
            Self::AuthError => "auth_error",
            Self::NetworkError => "network_error",
            Self::StateMismatch => "state_mismatch",
            Self::CannotCalculateChanges => "cannot_calculate_changes",
            Self::GatewayRejected => "gateway_rejected",
            Self::SecretUnavailable => "secret_unavailable",
            Self::SecretUnsupported => "secret_unsupported",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::StorageFailure => "storage_failure",
            Self::ConfigValidation => "config_validation",
            Self::ConfigIo => "config_io",
            Self::ConfigParse => "config_parse",
        }
    }
}

impl ServiceError {
    /// Returns the stable category used for API status mapping.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn kind(&self) -> ServiceErrorKind {
        match self {
            Self::Gateway(GatewayError::Unavailable(_)) => ServiceErrorKind::GatewayUnavailable,
            Self::Gateway(GatewayError::Auth) => ServiceErrorKind::AuthError,
            Self::Gateway(GatewayError::Network(_)) => ServiceErrorKind::NetworkError,
            Self::Gateway(GatewayError::StateMismatch) => ServiceErrorKind::StateMismatch,
            Self::Gateway(GatewayError::CannotCalculateChanges) => {
                ServiceErrorKind::CannotCalculateChanges
            }
            Self::Gateway(GatewayError::Rejected(_)) => ServiceErrorKind::GatewayRejected,
            Self::Secret(SecretStoreError::Unavailable(_)) => ServiceErrorKind::SecretUnavailable,
            Self::Secret(SecretStoreError::Unsupported(_)) => ServiceErrorKind::SecretUnsupported,
            Self::Store(StoreError::NotFound(_)) | Self::Config(ConfigError::NotFound(_)) => {
                ServiceErrorKind::NotFound
            }
            Self::Store(StoreError::Conflict(_)) | Self::Config(ConfigError::Conflict(_)) => {
                ServiceErrorKind::Conflict
            }
            Self::Store(StoreError::Failure(_)) => ServiceErrorKind::StorageFailure,
            Self::Config(ConfigError::Validation(_)) => ServiceErrorKind::ConfigValidation,
            Self::Config(ConfigError::Io(_)) => ServiceErrorKind::ConfigIo,
            Self::Config(ConfigError::Parse(_)) => ServiceErrorKind::ConfigParse,
        }
    }

    /// Returns the error code string used in the JSON error response body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn code(&self) -> &'static str {
        self.kind().code()
    }
}

/// Errors from credential storage operations.
///
/// @spec docs/L1-api#secret-management
#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret unavailable: {0}")]
    Unavailable(String),
    #[error("secret store does not support operation: {0}")]
    Unsupported(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_secret() -> SecretStatus {
        SecretStatus {
            storage: SecretStorage::Os,
            configured: true,
            label: None,
        }
    }

    #[test]
    fn message_event_topics_preserve_serialized_strings() {
        assert_eq!(
            EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED,
            "message.keywords_changed"
        );
        assert_eq!(EVENT_TOPIC_MESSAGE_BODY_CACHED, "message.body_cached");
        assert_eq!(
            EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
            "message.mailboxes_changed"
        );
    }

    #[test]
    fn account_driver_capabilities_match_cache_and_push_behavior() {
        assert_eq!(
            AccountDriver::Jmap.capabilities(),
            AccountDriverCapabilities {
                cache_fetch_unit: crate::CacheFetchUnit::BodyOnly,
                supports_push: true,
            }
        );
        assert_eq!(
            AccountDriver::ImapSmtp.capabilities(),
            AccountDriverCapabilities {
                cache_fetch_unit: crate::CacheFetchUnit::RawMessage,
                supports_push: false,
            }
        );
        assert_eq!(
            AccountDriver::Mock.capabilities(),
            AccountDriverCapabilities {
                cache_fetch_unit: crate::CacheFetchUnit::BodyOnly,
                supports_push: false,
            }
        );
    }

    #[test]
    fn service_error_kind_preserves_existing_codes() {
        let cases = [
            (
                ServiceError::from(GatewayError::Auth),
                ServiceErrorKind::AuthError,
                "auth_error",
            ),
            (
                ServiceError::from(GatewayError::StateMismatch),
                ServiceErrorKind::StateMismatch,
                "state_mismatch",
            ),
            (
                ServiceError::from(GatewayError::Network("timeout".to_string())),
                ServiceErrorKind::NetworkError,
                "network_error",
            ),
            (
                ServiceError::from(SecretStoreError::Unsupported("os".to_string())),
                ServiceErrorKind::SecretUnsupported,
                "secret_unsupported",
            ),
            (
                ServiceError::from(StoreError::NotFound("message:1".to_string())),
                ServiceErrorKind::NotFound,
                "not_found",
            ),
            (
                ServiceError::from(StoreError::Failure("disk full".to_string())),
                ServiceErrorKind::StorageFailure,
                "storage_failure",
            ),
            (
                ServiceError::from(ConfigError::Validation("bad source".to_string())),
                ServiceErrorKind::ConfigValidation,
                "config_validation",
            ),
            (
                ServiceError::from(ConfigError::Io("denied".to_string())),
                ServiceErrorKind::ConfigIo,
                "config_io",
            ),
        ];

        for (error, kind, code) in cases {
            assert_eq!(error.kind(), kind);
            assert_eq!(error.code(), code);
            assert_eq!(error.kind().code(), code);
        }
    }

    #[test]
    fn message_record_deserializes_without_recipients() {
        let record: MessageRecord = serde_json::from_value(serde_json::json!({
            "id": "message-1",
            "sourceThreadId": "thread-1",
            "remoteBlobId": null,
            "subject": "Legacy message",
            "fromName": null,
            "fromEmail": "sender@example.com",
            "preview": null,
            "receivedAt": "2026-05-24T00:00:00Z",
            "hasAttachment": false,
            "size": 0,
            "mailboxIds": [],
            "keywords": [],
            "bodyHtml": null,
            "bodyText": null,
            "rawMime": null,
            "rfcMessageId": null,
            "inReplyTo": null,
            "references": []
        }))
        .expect("legacy message record should deserialize");

        assert!(record.to.is_empty());
    }

    #[test]
    fn manual_connection_overview_serializes_editable_credentials_variant() {
        let value = serde_json::to_value(AccountConnectionOverview::ManualCredentials {
            provider: ProviderHint::Generic,
            provider_kind: ProviderKind::Generic,
            auth: ProviderAuthKind::AppPassword,
            base_url: Some("https://mail.example.com/jmap".to_string()),
            username: Some("me@example.com".to_string()),
            imap: None,
            smtp: None,
            secret: configured_secret(),
        })
        .expect("serialize connection overview");

        assert_eq!(value["kind"], "manualCredentials");
        assert_eq!(value["provider"], "generic");
        assert_eq!(value["providerKind"], "generic");
        assert_eq!(value["auth"], "appPassword");
        assert_eq!(value["baseUrl"], "https://mail.example.com/jmap");
        assert_eq!(value["username"], "me@example.com");
    }

    #[test]
    fn oauth_connection_overview_serializes_managed_variant_without_base_url() {
        let value = serde_json::to_value(AccountConnectionOverview::ManagedOAuth {
            provider: ProviderHint::Gmail,
            provider_kind: ProviderKind::Gmail,
            auth: ProviderAuthKind::OAuth2,
            username: Some("me@gmail.com".to_string()),
            imap: None,
            smtp: None,
            secret: configured_secret(),
        })
        .expect("serialize connection overview");

        assert_eq!(value["kind"], "managedOAuth");
        assert_eq!(value["provider"], "gmail");
        assert_eq!(value["providerKind"], "gmail");
        assert_eq!(value["auth"], "oauth2");
        assert!(value.get("baseUrl").is_none());
    }

    #[test]
    fn connection_overview_deserializes_legacy_provider_without_provider_kind() {
        let value: AccountConnectionOverview = serde_json::from_value(serde_json::json!({
            "kind": "managedOAuth",
            "provider": "gmail",
            "auth": "oauth2",
            "username": "me@gmail.com",
            "imap": null,
            "smtp": null,
            "secret": {
                "storage": "os",
                "configured": true,
                "label": null
            }
        }))
        .expect("legacy connection overview should deserialize");

        match value {
            AccountConnectionOverview::ManagedOAuth { provider_kind, .. } => {
                assert_eq!(provider_kind, ProviderKind::Gmail);
            }
            AccountConnectionOverview::ManualCredentials { .. } => {
                panic!("expected managed OAuth variant");
            }
        }
    }
}
