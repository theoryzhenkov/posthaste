use super::*;
use uuid::Uuid;

/// Build an [`AccountOverview`] by enriching settings with runtime status
/// and secret metadata. Secret values are never included.
///
/// @spec docs/L1-api#accounts
/// @spec docs/L1-api#secret-management
pub(super) async fn account_overview(
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
        transport: account_transport_overview(&account),
        created_at: account.created_at.clone(),
        updated_at: account.updated_at.clone(),
        is_default: settings.default_account_id.as_ref() == Some(&account.id),
        runtime,
    }
}

/// Build the transport portion of an account overview with redacted secret status.
fn account_transport_overview(account: &AccountSettings) -> AccountTransportOverview {
    AccountTransportOverview {
        base_url: account.transport.base_url.clone(),
        username: account.transport.username.clone(),
        secret: secret_status(account.transport.secret_ref.as_ref()),
    }
}

/// Derive a redacted [`SecretStatus`] from a secret reference.
/// OS-kind secrets hide the key; env-kind secrets expose the variable name.
///
/// @spec docs/L1-api#secret-management
pub(super) fn secret_status(secret_ref: Option<&SecretRef>) -> SecretStatus {
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

/// Convert an API transport request into domain transport settings,
/// normalizing empty strings to `None`.
impl From<AccountTransportRequest> for posthaste_domain::AccountTransportSettings {
    fn from(value: AccountTransportRequest) -> Self {
        Self {
            base_url: normalize_optional(value.base_url),
            username: normalize_optional(value.username),
            secret_ref: None,
        }
    }
}

/// Execute a secret write instruction (keep/replace/clear) against the OS
/// keyring and update the account's `secret_ref` accordingly.
///
/// @spec docs/L1-api#secret-management
pub(super) fn apply_secret_instruction(
    state: &AppState,
    account: &mut AccountSettings,
    previous_secret_ref: Option<&SecretRef>,
    secret: &SecretWriteRequest,
) -> Result<(), ApiError> {
    validate_secret_request(secret)?;

    match secret.mode {
        SecretWriteMode::Keep => {
            if let Some(previous_secret_ref) = previous_secret_ref {
                account.transport.secret_ref = Some(previous_secret_ref.clone());
            }
        }
        SecretWriteMode::Replace => {
            let password = required_secret_password(secret)?;
            let secret_ref = previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .unwrap_or_else(|| account_secret_ref(&account.id));
            match previous_secret_ref {
                Some(existing) if existing == &secret_ref => state
                    .secret_store
                    .update(&secret_ref, password)
                    .map_err(ServiceError::from)
                    .map_err(ApiError::from)?,
                _ => state
                    .secret_store
                    .save(&secret_ref, password)
                    .map_err(ServiceError::from)
                    .map_err(ApiError::from)?,
            }
            account.transport.secret_ref = Some(secret_ref);
        }
        SecretWriteMode::Clear => {
            delete_managed_secret(state, previous_secret_ref)?;
            account.transport.secret_ref = None;
        }
    }

    Ok(())
}

/// Validate that the secret write request is internally consistent
/// (e.g. `replace` requires a password, `keep`/`clear` forbid one).
///
/// @spec docs/L1-api#secret-management
pub(super) fn validate_secret_request(secret: &SecretWriteRequest) -> Result<(), ApiError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_secret",
                    "secret.password is only allowed when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Replace => {
            required_secret_password(secret)?;
        }
        SecretWriteMode::Clear => {
            if secret.password.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_secret",
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

/// Extract a non-empty password from the request, returning an error if missing.
fn required_secret_password(secret: &SecretWriteRequest) -> Result<&str, ApiError> {
    secret
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_secret",
                "secret.password is required when secret.mode is replace",
            )
        })
}

/// Validate required fields for an account: non-empty ID and name, plus
/// base URL and configured secret for JMAP accounts.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub(super) fn validate_account_settings(account: &AccountSettings) -> Result<(), ApiError> {
    if account.id.as_str().trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account id is required",
        ));
    }
    if account.name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account name is required",
        ));
    }
    if account
        .email_patterns
        .iter()
        .any(|pattern| pattern.trim().is_empty())
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "email patterns must not be blank",
        ));
    }
    if matches!(account.driver, AccountDriver::Jmap) {
        if account
            .transport
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP base URL is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP secret must be configured before saving the account",
            ));
        }
    }
    Ok(())
}

/// Build the default OS keyring secret reference for an account (`account:{id}`).
fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

/// Delete an OS-managed secret from the keyring. No-ops for env secrets.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub(super) fn delete_managed_secret(
    state: &AppState,
    secret_ref: Option<&SecretRef>,
) -> Result<(), ApiError> {
    if let Some(secret_ref) = secret_ref {
        if matches!(secret_ref.kind, SecretKind::Os) {
            state
                .secret_store
                .delete(secret_ref)
                .map_err(ServiceError::from)
                .map_err(ApiError::from)?;
        }
    }
    Ok(())
}

/// Trim whitespace from an optional string, converting empty/blank to `None`.
pub(super) fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Sparse-merge patch fields into an existing account. Omitted fields
/// (including transport sub-fields) are preserved.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub(super) fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountRequest) {
    if let Some(name) = &request.name {
        account.name = name.trim().to_string();
    }
    if let Some(full_name) = &request.full_name {
        account.full_name = normalize_optional(Some(full_name.clone()));
    }
    if let Some(email_patterns) = &request.email_patterns {
        account.email_patterns = normalize_email_patterns(email_patterns);
    }
    if let Some(driver) = &request.driver {
        account.driver = driver.clone();
    }
    if let Some(enabled) = request.enabled {
        account.enabled = enabled;
    }
    if let Some(transport) = &request.transport {
        if transport.base_url.is_some() {
            account.transport.base_url = normalize_optional(transport.base_url.clone());
        }
        if transport.username.is_some() {
            account.transport.username = normalize_optional(transport.username.clone());
        }
    }
}

/// Normalize user-owned email addresses/patterns by trimming whitespace and
/// dropping empty entries. Patterns such as `*@example.com` are preserved.
pub(super) fn normalize_email_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

/// Append an account lifecycle event to the event log and broadcast it.
///
/// @spec docs/L1-sync#event-propagation
pub(super) fn append_and_publish_account_event(
    state: &Arc<AppState>,
    account_id: &AccountId,
    topic: &str,
) -> Result<(), posthaste_domain::StoreError> {
    let event = state.store.append_event(
        account_id,
        topic,
        None,
        None,
        json!({ "accountId": account_id.as_str() }),
    )?;
    state.publish_events(&[event]);
    Ok(())
}

/// Convert a store-level error into an API error.
pub(super) fn store_error_to_api(error: posthaste_domain::StoreError) -> ApiError {
    ApiError::from_service_error(ServiceError::from(error))
}

/// Construct a 500 Internal Server Error from a message string.
pub(super) fn internal_error(error: String) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", error)
}

/// Generate a smart mailbox ID from a human name: `sm-{slug}-{uuid}`.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub(super) fn generate_smart_mailbox_id(name: &str) -> String {
    let slug = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!(
        "sm-{}-{}",
        if slug.is_empty() {
            "mailbox"
        } else {
            slug.as_str()
        },
        Uuid::new_v4()
    )
}

/// Generate an internal account ID from identity fields. The ID is deliberately
/// hidden from the UI; it only needs to be stable after account creation.
pub(super) fn generate_account_id_seed(name: &str, email_patterns: &[String]) -> String {
    let seed = email_patterns
        .iter()
        .map(|pattern| pattern.trim())
        .find(|pattern| !pattern.is_empty())
        .unwrap_or_else(|| name.trim());
    let slug = seed
        .trim_start_matches("*@")
        .trim_start_matches('@')
        .to_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        "account".to_string()
    } else {
        slug
    }
}
