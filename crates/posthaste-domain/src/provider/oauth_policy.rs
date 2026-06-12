use super::*;

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
