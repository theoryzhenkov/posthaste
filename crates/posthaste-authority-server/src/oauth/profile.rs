use super::*;

/// OAuth 2.0 provider endpoints and default mail scopes.
///
/// The flow follows the OAuth 2.1 security posture before OAuth 2.1 is final:
/// authorization code only, PKCE required, no password or implicit grant.
///
/// @spec docs/L0-providers#authentication
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthProviderProfile {
    pub provider: ProviderHint,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub metadata_url: &'static str,
    pub scopes: &'static [&'static str],
    pub extra_authorization_params: &'static [(&'static str, &'static str)],
}

impl OAuthProviderProfile {
    pub fn for_provider(provider: &ProviderHint) -> Option<Self> {
        let provider_profile = ProviderProfile::from_hint(provider);
        if !provider_profile.oauth().is_supported() {
            return None;
        }

        match provider_profile.kind() {
            ProviderKind::Gmail => Some(Self {
                provider: provider.clone(),
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
                token_url: "https://oauth2.googleapis.com/token",
                metadata_url: "https://accounts.google.com/.well-known/openid-configuration",
                scopes: &["openid", "email", "https://mail.google.com/"],
                extra_authorization_params: &[("access_type", "offline"), ("prompt", "consent")],
            }),
            ProviderKind::Outlook => Some(Self {
                provider: provider.clone(),
                auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                metadata_url:
                    "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration",
                scopes: &[
                    "openid",
                    "email",
                    "offline_access",
                    "https://outlook.office.com/IMAP.AccessAsUser.All",
                    "https://outlook.office.com/SMTP.Send",
                ],
                extra_authorization_params: &[],
            }),
            ProviderKind::Generic | ProviderKind::Icloud => None,
        }
    }

    pub fn openid_issuer_matches(&self, issuer: &str) -> bool {
        ProviderProfile::from_hint(&self.provider)
            .oauth()
            .openid_issuer_matches(issuer)
    }

    pub fn default_mail_transport(&self) -> Option<(ImapTransportSettings, SmtpTransportSettings)> {
        ProviderProfile::from_hint(&self.provider)
            .oauth()
            .default_mail_transport()
    }
}
