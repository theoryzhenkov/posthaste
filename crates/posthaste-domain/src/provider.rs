use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    AccountSettings, AccountTransportSettings, ImapCapabilities, ImapFullSyncReason,
    ImapLabelSource, ImapMessageIdentitySource, ImapProviderFeatures, ImapThreadIdentitySource,
    ImapTransportSettings, MailboxRole, ProviderHint, SmtpTransportSettings, TransportSecurity,
};

/// Provider family independent of the account driver/protocol.
///
/// `AccountDriver` selects the runtime protocol. `ProviderKind` selects the
/// vendor/family policy applied within that protocol.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProviderKind {
    #[default]
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

impl ProviderKind {
    pub fn from_imap_capabilities(capabilities: &ImapCapabilities) -> Self {
        if capabilities.supports_gmail_extensions() {
            Self::Gmail
        } else {
            Self::Generic
        }
    }
}

impl From<&ProviderHint> for ProviderKind {
    fn from(provider: &ProviderHint) -> Self {
        match provider {
            ProviderHint::Generic => Self::Generic,
            ProviderHint::Gmail => Self::Gmail,
            ProviderHint::Outlook => Self::Outlook,
            ProviderHint::Icloud => Self::Icloud,
        }
    }
}

impl From<ProviderHint> for ProviderKind {
    fn from(provider: ProviderHint) -> Self {
        Self::from(&provider)
    }
}

impl AccountTransportSettings {
    pub fn provider_profile(&self) -> ProviderProfile {
        ProviderProfile::from_hint(&self.provider)
    }
}

impl AccountSettings {
    pub fn provider_profile(&self) -> ProviderProfile {
        self.transport.provider_profile()
    }
}

/// Provider profile selected for one account or discovered provider family.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderProfile {
    kind: ProviderKind,
    policy: ProviderPolicy,
}

impl ProviderProfile {
    pub fn from_hint(provider: &ProviderHint) -> Self {
        Self::from_kind(ProviderKind::from(provider))
    }

    pub fn from_kind(kind: ProviderKind) -> Self {
        Self {
            kind,
            policy: ProviderPolicy::for_kind(kind),
        }
    }

    pub fn from_imap_capabilities(capabilities: &ImapCapabilities) -> Self {
        Self::from_kind(ProviderKind::from_imap_capabilities(capabilities))
    }

    pub fn kind(self) -> ProviderKind {
        self.kind
    }

    pub fn policy(self) -> ProviderPolicy {
        self.policy
    }

    pub fn jmap(self) -> JmapProviderPolicy {
        self.policy.jmap
    }

    pub fn imap(self) -> ImapProviderPolicy {
        self.policy.imap
    }

    pub fn smtp(self) -> SmtpProviderPolicy {
        self.policy.smtp
    }

    pub fn oauth(self) -> OAuthProviderPolicy {
        self.policy.oauth
    }
}

/// Protocol-specific provider policies grouped under one provider boundary.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderPolicy {
    pub jmap: JmapProviderPolicy,
    pub imap: ImapProviderPolicy,
    pub smtp: SmtpProviderPolicy,
    pub oauth: OAuthProviderPolicy,
}

impl ProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        Self {
            jmap: JmapProviderPolicy::for_kind(kind),
            imap: ImapProviderPolicy::for_kind(kind),
            smtp: SmtpProviderPolicy::for_kind(kind),
            oauth: OAuthProviderPolicy::for_kind(kind),
        }
    }
}

/// JMAP provider policy placeholder.
///
/// JMAP currently has no vendor-specific behavior in the domain layer, but it
/// still participates in the same profile boundary as IMAP/SMTP.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JmapProviderPolicy {
    remote_observation: RemoteObservationPolicy,
}

impl JmapProviderPolicy {
    pub fn for_kind(_kind: ProviderKind) -> Self {
        Self {
            remote_observation: RemoteObservationPolicy::account_push(),
        }
    }

    pub fn remote_observation(self) -> RemoteObservationPolicy {
        self.remote_observation
    }
}

/// IMAP provider policy selected for mailbox planning and projection.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapProviderPolicy {
    features: ImapProviderFeatures,
    required_full_sync_reason: Option<ImapFullSyncReason>,
    remote_observation: RemoteObservationPolicy,
    mailbox_role_aliases: ImapMailboxRoleAliasPolicy,
}

impl ImapProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Gmail => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::GmailMessageId,
                    thread_identity: ImapThreadIdentitySource::GmailThreadId,
                    label_source: ImapLabelSource::GmailLabels,
                },
                required_full_sync_reason: Some(
                    ImapFullSyncReason::ProviderCanonicalizationRequired,
                ),
                remote_observation: RemoteObservationPolicy::selected_mailbox_idle()
                    .with_incomplete_hints(),
                mailbox_role_aliases: ImapMailboxRoleAliasPolicy::Gmail,
            },
            ProviderKind::Generic | ProviderKind::Outlook | ProviderKind::Icloud => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::UidValidityUid,
                    thread_identity: ImapThreadIdentitySource::Rfc5322Headers,
                    label_source: ImapLabelSource::MailboxMembership,
                },
                required_full_sync_reason: None,
                remote_observation: RemoteObservationPolicy::selected_mailbox_idle(),
                mailbox_role_aliases: ImapMailboxRoleAliasPolicy::StandardOnly,
            },
        }
    }

    pub fn features(self) -> ImapProviderFeatures {
        self.features
    }

    pub fn required_full_sync_reason(self) -> Option<ImapFullSyncReason> {
        self.required_full_sync_reason
    }

    pub fn allows_status_skip(self) -> bool {
        self.required_full_sync_reason.is_none()
    }

    pub fn canonicalizes_by_rfc5322_message_id(self) -> bool {
        self.features.message_identity == ImapMessageIdentitySource::Rfc5322MessageId
    }

    pub fn canonicalizes_by_gmail_message_id(self) -> bool {
        self.features.message_identity == ImapMessageIdentitySource::GmailMessageId
    }

    pub fn remote_observation(self) -> RemoteObservationPolicy {
        self.remote_observation
    }

    pub fn mailbox_role(
        self,
        mailbox_name: &str,
        attributes: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Option<&'static str> {
        let normalized = attributes
            .into_iter()
            .map(|attribute| attribute.as_ref().to_ascii_uppercase())
            .collect::<BTreeSet<_>>();

        crate::imap_special_use_role(mailbox_name, normalized.iter().map(String::as_str)).or_else(
            || match self.mailbox_role_aliases {
                ImapMailboxRoleAliasPolicy::StandardOnly => None,
                ImapMailboxRoleAliasPolicy::Gmail => gmail_mailbox_role_alias(&normalized),
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImapMailboxRoleAliasPolicy {
    StandardOnly,
    Gmail,
}

fn gmail_mailbox_role_alias(attributes: &BTreeSet<String>) -> Option<&'static str> {
    if attributes.contains("\\SPAM") {
        Some(MailboxRole::Junk.as_str())
    } else if attributes.contains("\\ALL") {
        Some(MailboxRole::Archive.as_str())
    } else {
        None
    }
}

/// Provider policy for turning remote push/IDLE signals into local sync work.
///
/// Push transports only deliver hints. The policy records what remote surface
/// is observed and whether a hint must be followed by an account-level
/// observation rather than trusted as a complete change description.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteObservationPolicy {
    idle_scope: RemoteIdleScope,
    empty_hint: EmptyRemoteHintPolicy,
    hint_completeness: RemoteHintCompleteness,
}

impl RemoteObservationPolicy {
    pub fn account_push() -> Self {
        Self {
            idle_scope: RemoteIdleScope::Account,
            empty_hint: EmptyRemoteHintPolicy::Ignore,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn selected_mailbox_idle() -> Self {
        Self {
            idle_scope: RemoteIdleScope::SelectedMailbox,
            empty_hint: EmptyRemoteHintPolicy::Sync,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn disabled() -> Self {
        Self {
            idle_scope: RemoteIdleScope::None,
            empty_hint: EmptyRemoteHintPolicy::Ignore,
            hint_completeness: RemoteHintCompleteness::Complete,
        }
    }

    pub fn with_incomplete_hints(mut self) -> Self {
        self.hint_completeness = RemoteHintCompleteness::Incomplete;
        self
    }

    pub fn idle_scope(self) -> RemoteIdleScope {
        self.idle_scope
    }

    pub fn observes_empty_hints(self) -> bool {
        self.empty_hint == EmptyRemoteHintPolicy::Sync
    }

    pub fn treats_hints_as_incomplete(self) -> bool {
        self.hint_completeness == RemoteHintCompleteness::Incomplete
    }
}

impl Default for RemoteObservationPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteIdleScope {
    None,
    Account,
    SelectedMailbox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmptyRemoteHintPolicy {
    Ignore,
    Sync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteHintCompleteness {
    Complete,
    Incomplete,
}

/// SMTP provider policy selected after message submission.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmtpProviderPolicy {
    sent_copy: SmtpSentCopyPolicy,
}

impl SmtpProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        let sent_copy = match kind {
            ProviderKind::Gmail | ProviderKind::Outlook => SmtpSentCopyPolicy::ProviderManaged,
            ProviderKind::Generic | ProviderKind::Icloud => SmtpSentCopyPolicy::AppendToSentMailbox,
        };
        Self { sent_copy }
    }

    pub fn sent_copy(self) -> SmtpSentCopyPolicy {
        self.sent_copy
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpSentCopyPolicy {
    ProviderManaged,
    AppendToSentMailbox,
}

/// OAuth provider policy shared by API setup and token validation.
///
/// Server-specific authorization URLs and scopes stay in the server adapter,
/// while provider eligibility, OIDC issuer policy, and default IMAP/SMTP
/// endpoints are selected through the general provider profile.
///
/// @spec docs/L0-providers#authentication
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OAuthProviderPolicy {
    openid_issuer: OAuthOpenIdIssuerPolicy,
    default_mail_transport: Option<OAuthDefaultMailTransport>,
}

impl OAuthProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Gmail => Self {
                openid_issuer: OAuthOpenIdIssuerPolicy::Google,
                default_mail_transport: Some(OAuthDefaultMailTransport::Gmail),
            },
            ProviderKind::Outlook => Self {
                openid_issuer: OAuthOpenIdIssuerPolicy::MicrosoftTenantV2,
                default_mail_transport: Some(OAuthDefaultMailTransport::Outlook),
            },
            ProviderKind::Generic | ProviderKind::Icloud => Self {
                openid_issuer: OAuthOpenIdIssuerPolicy::Unsupported,
                default_mail_transport: None,
            },
        }
    }

    pub fn is_supported(self) -> bool {
        self.default_mail_transport.is_some()
            && self.openid_issuer != OAuthOpenIdIssuerPolicy::Unsupported
    }

    pub fn openid_issuer_matches(self, issuer: &str) -> bool {
        self.openid_issuer.matches(issuer)
    }

    pub fn default_mail_transport(self) -> Option<(ImapTransportSettings, SmtpTransportSettings)> {
        self.default_mail_transport
            .map(OAuthDefaultMailTransport::to_settings)
    }

    pub fn openid_issuer_policy(self) -> OAuthOpenIdIssuerPolicy {
        self.openid_issuer
    }

    pub fn default_mail_transport_policy(self) -> Option<OAuthDefaultMailTransport> {
        self.default_mail_transport
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OAuthOpenIdIssuerPolicy {
    Google,
    MicrosoftTenantV2,
    Unsupported,
}

impl OAuthOpenIdIssuerPolicy {
    pub fn matches(self, issuer: &str) -> bool {
        match self {
            Self::Google => {
                issuer == "https://accounts.google.com" || issuer == "accounts.google.com"
            }
            Self::MicrosoftTenantV2 => {
                issuer.starts_with("https://login.microsoftonline.com/")
                    && issuer.ends_with("/v2.0")
            }
            Self::Unsupported => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OAuthDefaultMailTransport {
    Gmail,
    Outlook,
}

impl OAuthDefaultMailTransport {
    pub fn to_settings(self) -> (ImapTransportSettings, SmtpTransportSettings) {
        match self {
            Self::Gmail => (
                ImapTransportSettings {
                    host: "imap.gmail.com".to_string(),
                    port: 993,
                    security: TransportSecurity::Tls,
                },
                SmtpTransportSettings {
                    host: "smtp.gmail.com".to_string(),
                    port: 587,
                    security: TransportSecurity::StartTls,
                },
            ),
            Self::Outlook => (
                ImapTransportSettings {
                    host: "outlook.office365.com".to_string(),
                    port: 993,
                    security: TransportSecurity::Tls,
                },
                SmtpTransportSettings {
                    host: "smtp.office365.com".to_string(),
                    port: 587,
                    security: TransportSecurity::StartTls,
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests;
