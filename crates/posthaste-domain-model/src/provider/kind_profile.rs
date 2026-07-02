use super::*;

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
