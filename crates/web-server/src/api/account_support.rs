use super::*;
use uuid::Uuid;

pub(super) async fn account_overview(
    state: &Arc<AppState>,
    settings: &AppSettings,
    account: AccountSettings,
) -> AccountOverview {
    let runtime = state.supervisor.runtime_overview(&account.id).await;
    AccountOverview {
        id: account.id.clone(),
        name: account.name.clone(),
        driver: account.driver.clone(),
        enabled: account.enabled,
        transport: account_transport_overview(&account),
        created_at: account.created_at.clone(),
        updated_at: account.updated_at.clone(),
        is_default: settings.default_account_id.as_ref() == Some(&account.id),
        runtime,
    }
}

fn account_transport_overview(account: &AccountSettings) -> AccountTransportOverview {
    AccountTransportOverview {
        base_url: account.transport.base_url.clone(),
        username: account.transport.username.clone(),
        secret: secret_status(account.transport.secret_ref.as_ref()),
    }
}

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

impl From<AccountTransportRequest> for mail_domain::AccountTransportSettings {
    fn from(value: AccountTransportRequest) -> Self {
        Self {
            base_url: normalize_optional(value.base_url),
            username: normalize_optional(value.username),
            secret_ref: None,
        }
    }
}

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
        if account
            .transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP password must be configured before saving the account",
            ));
        }
    }
    Ok(())
}

fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

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

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(super) fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountRequest) {
    if let Some(name) = &request.name {
        account.name = name.clone();
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

pub(super) fn append_and_publish_account_event(
    state: &Arc<AppState>,
    account_id: &AccountId,
    topic: &str,
) -> Result<(), mail_domain::StoreError> {
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

pub(super) fn store_error_to_api(error: mail_domain::StoreError) -> ApiError {
    ApiError::from_service_error(ServiceError::from(error))
}

pub(super) fn internal_error(error: String) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", error)
}

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
