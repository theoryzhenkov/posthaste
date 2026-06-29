use super::*;

impl SourceToml {
    /// Converts this TOML struct to the domain `AccountSettings`. Missing
    /// timestamps default to `RFC3339_EPOCH`.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn to_account_settings(&self) -> Result<AccountSettings, String> {
        let settings = AccountSettings {
            id: AccountId::from(self.id.as_str()),
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            email_patterns: self.email_patterns.clone(),
            driver: self.driver.to_domain(),
            enabled: self.enabled,
            appearance: self.appearance.as_ref().map(|appearance| match appearance {
                AccountAppearanceToml::Initials {
                    initials,
                    color_hue,
                } => AccountAppearance::Initials {
                    initials: initials.clone(),
                    color_hue: *color_hue,
                },
                AccountAppearanceToml::Image {
                    image_id,
                    initials,
                    color_hue,
                } => AccountAppearance::Image {
                    image_id: image_id.clone(),
                    initials: initials.clone(),
                    color_hue: *color_hue,
                },
            }),
            transport: AccountTransportSettings {
                provider: convert_provider_hint(&self.transport.provider),
                auth: convert_auth_kind(&self.transport.auth),
                base_url: self.transport.base_url.clone(),
                username: self.transport.username.clone(),
                secret_ref: self.transport.secret_ref.as_ref().map(|sr| SecretRef {
                    kind: sr.kind.to_domain(),
                    key: sr.key.clone(),
                }),
                imap: self
                    .transport
                    .imap
                    .as_ref()
                    .map(|imap| ImapTransportSettings {
                        host: imap.host.clone(),
                        port: imap.port,
                        security: convert_transport_security(&imap.security),
                    }),
                smtp: self
                    .transport
                    .smtp
                    .as_ref()
                    .map(|smtp| SmtpTransportSettings {
                        host: smtp.host.clone(),
                        port: smtp.port,
                        security: convert_transport_security(&smtp.security),
                    }),
            },
            created_at: self
                .created_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
            updated_at: self
                .updated_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
        };
        validate_source_settings(&settings)?;
        Ok(settings)
    }

    /// Builds a `SourceToml` from domain `AccountSettings` for serialization.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn from_account_settings(settings: &AccountSettings) -> Self {
        Self {
            id: settings.id.to_string(),
            name: settings.name.clone(),
            full_name: settings.full_name.clone(),
            email_patterns: settings.email_patterns.clone(),
            driver: DriverToml::from_domain(&settings.driver),
            enabled: settings.enabled,
            appearance: settings
                .appearance
                .as_ref()
                .map(|appearance| match appearance {
                    AccountAppearance::Initials {
                        initials,
                        color_hue,
                    } => AccountAppearanceToml::Initials {
                        initials: initials.clone(),
                        color_hue: *color_hue,
                    },
                    AccountAppearance::Image {
                        image_id,
                        initials,
                        color_hue,
                    } => AccountAppearanceToml::Image {
                        image_id: image_id.clone(),
                        initials: initials.clone(),
                        color_hue: *color_hue,
                    },
                }),
            transport: TransportToml {
                provider: convert_provider_hint_to_toml(&settings.transport.provider),
                auth: convert_auth_kind_to_toml(&settings.transport.auth),
                base_url: settings.transport.base_url.clone(),
                username: settings.transport.username.clone(),
                secret_ref: settings
                    .transport
                    .secret_ref
                    .as_ref()
                    .map(|sr| SecretRefToml {
                        kind: SecretKindToml::from_domain(&sr.kind),
                        key: sr.key.clone(),
                    }),
                imap: settings
                    .transport
                    .imap
                    .as_ref()
                    .map(|imap| ImapTransportToml {
                        host: imap.host.clone(),
                        port: imap.port,
                        security: convert_transport_security_to_toml(&imap.security),
                    }),
                smtp: settings
                    .transport
                    .smtp
                    .as_ref()
                    .map(|smtp| SmtpTransportToml {
                        host: smtp.host.clone(),
                        port: smtp.port,
                        security: convert_transport_security_to_toml(&smtp.security),
                    }),
            },
            created_at: Some(settings.created_at.clone()),
            updated_at: Some(settings.updated_at.clone()),
        }
    }
}

pub(crate) fn convert_provider_hint(provider: &ProviderHintToml) -> ProviderHint {
    provider.to_domain()
}

pub(crate) fn convert_provider_hint_to_toml(provider: &ProviderHint) -> ProviderHintToml {
    ProviderHintToml::from_domain(provider)
}

pub(crate) fn convert_auth_kind(auth: &ProviderAuthKindToml) -> ProviderAuthKind {
    auth.to_domain()
}

pub(crate) fn convert_auth_kind_to_toml(auth: &ProviderAuthKind) -> ProviderAuthKindToml {
    ProviderAuthKindToml::from_domain(auth)
}

pub(crate) fn convert_transport_security(security: &TransportSecurityToml) -> TransportSecurity {
    security.to_domain()
}

pub(crate) fn convert_transport_security_to_toml(
    security: &TransportSecurity,
) -> TransportSecurityToml {
    TransportSecurityToml::from_domain(security)
}

pub(crate) fn validate_source_settings(settings: &AccountSettings) -> Result<(), String> {
    if !matches!(settings.driver, AccountDriver::ImapSmtp) {
        return Ok(());
    }
    if settings
        .transport
        .username
        .as_deref()
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .is_none()
    {
        return Err("imap_smtp source requires transport.username".to_string());
    }
    if settings.transport.secret_ref.is_none() {
        return Err("imap_smtp source requires transport.secret_ref".to_string());
    }
    if settings.transport.imap.is_none() {
        return Err("imap_smtp source requires transport.imap".to_string());
    }
    if settings.transport.smtp.is_none() {
        return Err("imap_smtp source requires transport.smtp".to_string());
    }
    if !settings.email_patterns.iter().any(|pattern| {
        let trimmed = pattern.trim();
        !trimmed.contains('*') && trimmed.contains('@')
    }) {
        return Err("imap_smtp source requires a concrete sender email pattern".to_string());
    }
    Ok(())
}
