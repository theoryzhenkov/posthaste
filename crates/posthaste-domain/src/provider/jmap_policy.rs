use super::*;

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
