use serde::{Deserialize, Serialize};

use crate::{
    AccountSettings, AccountTransportSettings, ImapCapabilities, ImapFullSyncReason,
    ImapLabelSource, ImapMessageIdentitySource, ImapProviderFeatures, ImapThreadIdentitySource,
    ProviderHint,
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
}

/// Protocol-specific provider policies grouped under one provider boundary.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderPolicy {
    pub jmap: JmapProviderPolicy,
    pub imap: ImapProviderPolicy,
    pub smtp: SmtpProviderPolicy,
}

impl ProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        Self {
            jmap: JmapProviderPolicy::for_kind(kind),
            imap: ImapProviderPolicy::for_kind(kind),
            smtp: SmtpProviderPolicy::for_kind(kind),
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
        assert_eq!(
            profile.smtp().sent_copy(),
            SmtpSentCopyPolicy::AppendToSentMailbox
        );
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
}
