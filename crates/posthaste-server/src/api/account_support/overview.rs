use super::*;

/// Build an [`AccountOverview`] by enriching settings with runtime status
/// and secret metadata. Secret values are never included.
///
/// @spec docs/L1-api#accounts
/// @spec docs/L1-api#secret-management
pub(crate) async fn account_overview(
    state: &Arc<AppState>,
    settings: &AppSettings,
    account: AccountSettings,
) -> AccountOverview {
    let runtime = state.supervisor.runtime_overview(&account.id).await;
    AccountOverview {
        id: account.id.clone(),
        name: account.name.clone(),
        full_name: account.full_name.clone(),
        email_patterns: account.email_patterns.clone(),
        driver: account.driver.clone(),
        enabled: account.enabled,
        appearance: account
            .appearance
            .clone()
            .map(normalize_account_appearance)
            .unwrap_or_else(|| default_account_appearance(&account)),
        connection: account_connection_overview(&account),
        created_at: account.created_at.clone(),
        updated_at: account.updated_at.clone(),
        is_default: settings.default_account_id.as_ref() == Some(&account.id),
        runtime,
    }
}

/// Build the account connection variant from persisted transport settings.
fn account_connection_overview(account: &AccountSettings) -> AccountConnectionOverview {
    let secret = secret_status(account.transport.secret_ref.as_ref());
    match account.transport.auth {
        ProviderAuthKind::OAuth2 => AccountConnectionOverview::ManagedOAuth {
            provider: account.transport.provider.clone(),
            provider_kind: account.transport.provider_kind(),
            auth: account.transport.auth.clone(),
            username: account.transport.username.clone(),
            imap: account.transport.imap.clone(),
            smtp: account.transport.smtp.clone(),
            secret,
        },
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            AccountConnectionOverview::ManualCredentials {
                provider: account.transport.provider.clone(),
                provider_kind: account.transport.provider_kind(),
                auth: account.transport.auth.clone(),
                base_url: account.transport.base_url.clone(),
                username: account.transport.username.clone(),
                imap: account.transport.imap.clone(),
                smtp: account.transport.smtp.clone(),
                secret,
            }
        }
    }
}

/// Derive a redacted [`SecretStatus`] from a secret reference.
/// OS-kind secrets hide the key; env-kind secrets expose the variable name.
///
/// @spec docs/L1-api#secret-management
pub(crate) fn secret_status(secret_ref: Option<&SecretRef>) -> SecretStatus {
    match secret_ref {
        Some(secret_ref) => SecretStatus {
            storage: secret_ref.kind.clone(),
            configured: true,
            label: match secret_ref.kind {
                SecretKind::Env => Some(secret_ref.key.clone()),
                SecretKind::Os => None,
            },
        },
        None => SecretStatus {
            storage: SecretStorage::Os,
            configured: false,
            label: None,
        },
    }
}
