use super::*;

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
    /// Volatile runtime state, owned by the account supervisor. Nested (not
    /// flattened) so the UI can update config and runtime through independent
    /// paths without the two racing inside one flat object.
    pub runtime: AccountRuntimeOverview,
}
