use super::*;

/// Convert an API transport request into domain transport settings,
/// normalizing empty strings to `None`.
impl From<AccountTransportRequest> for posthaste_domain_service::AccountTransportSettings {
    fn from(value: AccountTransportRequest) -> Self {
        Self {
            provider: value.provider.unwrap_or_default(),
            auth: value.auth.unwrap_or_default(),
            base_url: normalize_optional(value.base_url),
            username: normalize_optional(value.username),
            secret_ref: None,
            imap: value.imap,
            smtp: value.smtp,
        }
    }
}
