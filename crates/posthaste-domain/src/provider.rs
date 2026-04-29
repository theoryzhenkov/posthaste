use serde::{Deserialize, Serialize};

use crate::{
    AccountSettings, AccountTransportSettings, ImapCapabilities, ImapFullSyncReason,
    ImapLabelSource, ImapMessageIdentitySource, ImapProviderFeatures, ImapThreadIdentitySource,
    ImapTransportSettings, ProviderHint, SmtpTransportSettings, TransportSecurity,
};

/// Provider family independent of the account driver/protocol.
///
/// `AccountDriver` selects the runtime protocol. `ProviderKind` selects the
/// vendor/family policy applied within that protocol.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JmapProviderPolicy;

impl JmapProviderPolicy {
    pub fn for_kind(_kind: ProviderKind) -> Self {
        Self
    }
}

/// IMAP provider policy selected for mailbox planning and projection.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapProviderPolicy {
    features: ImapProviderFeatures,
    required_full_sync_reason: Option<ImapFullSyncReason>,
}

impl ImapProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Gmail => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::Rfc5322MessageId,
                    thread_identity: ImapThreadIdentitySource::Rfc5322Headers,
                    label_source: ImapLabelSource::GmailLabels,
                },
                required_full_sync_reason: Some(
                    ImapFullSyncReason::ProviderCanonicalizationRequired,
                ),
            },
            ProviderKind::Generic | ProviderKind::Outlook | ProviderKind::Icloud => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::UidValidityUid,
                    thread_identity: ImapThreadIdentitySource::Rfc5322Headers,
                    label_source: ImapLabelSource::MailboxMembership,
                },
                required_full_sync_reason: None,
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
mod tests {
    use super::*;

    #[test]
    fn profile_from_hint_groups_protocol_policies_under_provider_kind() {
        let profile = ProviderProfile::from_hint(&ProviderHint::Gmail);

        assert_eq!(profile.kind(), ProviderKind::Gmail);
        assert_eq!(profile.jmap(), JmapProviderPolicy);
        assert_eq!(
            profile.imap().required_full_sync_reason(),
            Some(ImapFullSyncReason::ProviderCanonicalizationRequired)
        );
        assert_eq!(
            profile.smtp().sent_copy(),
            SmtpSentCopyPolicy::ProviderManaged
        );
    }

    #[test]
    fn profile_from_imap_capabilities_detects_gmail_policy() {
        let profile = ProviderProfile::from_imap_capabilities(&ImapCapabilities::from_tokens([
            "IMAP4rev1",
            "X-GM-EXT-1",
        ]));

        assert_eq!(profile.kind(), ProviderKind::Gmail);
        assert_eq!(
            profile.imap().features().message_identity,
            ImapMessageIdentitySource::Rfc5322MessageId
        );
    }

    #[test]
    fn generic_profile_uses_conservative_default_policies() {
        let profile = ProviderProfile::from_hint(&ProviderHint::Generic);

        assert_eq!(profile.kind(), ProviderKind::Generic);
        assert!(profile.imap().allows_status_skip());
        assert!(!profile.imap().canonicalizes_by_rfc5322_message_id());
        assert_eq!(
            profile.smtp().sent_copy(),
            SmtpSentCopyPolicy::AppendToSentMailbox
        );
    }

    #[test]
    fn imap_policy_exposes_rfc5322_canonicalization_without_vendor_match() {
        let profile = ProviderProfile::from_imap_capabilities(&ImapCapabilities::from_tokens([
            "IMAP4rev1",
            "X-GM-EXT-1",
        ]));

        assert!(profile.imap().canonicalizes_by_rfc5322_message_id());
    }

    #[test]
    fn account_transport_exposes_provider_profile_boundary() {
        let transport = AccountTransportSettings {
            provider: ProviderHint::Outlook,
            ..AccountTransportSettings::default()
        };

        assert_eq!(transport.provider_profile().kind(), ProviderKind::Outlook);
        assert_eq!(
            transport.provider_profile().smtp().sent_copy(),
            SmtpSentCopyPolicy::ProviderManaged
        );
    }

    #[test]
    fn oauth_policy_is_available_only_for_supported_provider_profiles() {
        let cases = [
            (ProviderHint::Gmail, ProviderKind::Gmail, true),
            (ProviderHint::Outlook, ProviderKind::Outlook, true),
            (ProviderHint::Generic, ProviderKind::Generic, false),
            (ProviderHint::Icloud, ProviderKind::Icloud, false),
        ];

        for (hint, kind, supported) in cases {
            let profile = ProviderProfile::from_hint(&hint);

            assert_eq!(profile.kind(), kind);
            assert_eq!(profile.oauth().is_supported(), supported);
            assert_eq!(
                profile.oauth().default_mail_transport().is_some(),
                supported
            );
        }
    }

    #[test]
    fn oauth_policy_matches_provider_issuer_rules() {
        let gmail = ProviderProfile::from_kind(ProviderKind::Gmail).oauth();
        let outlook = ProviderProfile::from_kind(ProviderKind::Outlook).oauth();
        let generic = ProviderProfile::from_kind(ProviderKind::Generic).oauth();

        assert!(gmail.openid_issuer_matches("https://accounts.google.com"));
        assert!(gmail.openid_issuer_matches("accounts.google.com"));
        assert!(!gmail.openid_issuer_matches("https://login.microsoftonline.com/tenant/v2.0"));
        assert!(outlook.openid_issuer_matches("https://login.microsoftonline.com/tenant/v2.0"));
        assert!(!outlook.openid_issuer_matches("https://accounts.google.com"));
        assert!(!generic.openid_issuer_matches("https://accounts.google.com"));
    }

    #[test]
    fn oauth_policy_provides_default_mail_endpoints() {
        let gmail = ProviderProfile::from_kind(ProviderKind::Gmail)
            .oauth()
            .default_mail_transport()
            .expect("Gmail OAuth mail transport");
        let outlook = ProviderProfile::from_kind(ProviderKind::Outlook)
            .oauth()
            .default_mail_transport()
            .expect("Outlook OAuth mail transport");

        assert_eq!(gmail.0.host, "imap.gmail.com");
        assert_eq!(gmail.0.security, TransportSecurity::Tls);
        assert_eq!(gmail.1.host, "smtp.gmail.com");
        assert_eq!(gmail.1.security, TransportSecurity::StartTls);
        assert_eq!(outlook.0.host, "outlook.office365.com");
        assert_eq!(outlook.1.host, "smtp.office365.com");
    }
}
