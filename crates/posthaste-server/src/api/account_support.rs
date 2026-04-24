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
        appearance: account
            .appearance
            .clone()
            .map(normalize_account_appearance)
            .unwrap_or_else(|| default_account_appearance(&account)),
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
    if let Some(appearance) = &account.appearance {
        validate_account_appearance(appearance)?;
    }
    Ok(())
}

pub(super) fn validate_automation_rules(rules: &[AutomationRule]) -> Result<(), ApiError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule ids must be unique",
            ));
        }
        if rule.name.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule name is required",
            ));
        }
        if rule.triggers.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule must include at least one trigger",
            ));
        }
        if rule.actions.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule must include at least one action",
            ));
        }
        for action in &rule.actions {
            match action {
                AutomationAction::ApplyTag { tag } | AutomationAction::RemoveTag { tag }
                    if tag.trim().is_empty() || tag.starts_with('$') =>
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_account",
                        "automation tag must be a non-system keyword",
                    ));
                }
                AutomationAction::MoveToMailbox { mailbox_id }
                    if mailbox_id.as_str().trim().is_empty() =>
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_account",
                        "automation target mailbox id is required",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Deterministic default visual identity for accounts without customization.
pub(super) fn default_account_appearance(account: &AccountSettings) -> AccountAppearance {
    AccountAppearance::Initials {
        initials: derive_account_initials(account),
        color_hue: account_color_hue(account),
    }
}

/// Normalize user-supplied appearance strings while preserving the selected mode.
pub(super) fn normalize_account_appearance(appearance: AccountAppearance) -> AccountAppearance {
    match appearance {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => AccountAppearance::Initials {
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
        AccountAppearance::Image {
            image_id,
            initials,
            color_hue,
        } => AccountAppearance::Image {
            image_id: image_id.trim().to_string(),
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
    }
}

fn validate_account_appearance(appearance: &AccountAppearance) -> Result<(), ApiError> {
    let (initials, color_hue, image_id) = match appearance {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => (initials, color_hue, None),
        AccountAppearance::Image {
            image_id,
            initials,
            color_hue,
        } => (initials, color_hue, Some(image_id)),
    };
    if initials.trim().is_empty() || initials.chars().count() > 4 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account appearance initials must be 1-4 characters",
        ));
    }
    if *color_hue > 360 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account appearance color hue must be between 0 and 360",
        ));
    }
    if let Some(image_id) = image_id {
        validate_logo_image_id(image_id)?;
    }
    Ok(())
}

pub(super) fn validate_logo_image_id(image_id: &str) -> Result<(), ApiError> {
    let is_valid = !image_id.is_empty()
        && image_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if !is_valid {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account_logo",
            "account logo image id is invalid",
        ));
    }
    Ok(())
}

fn derive_account_initials(account: &AccountSettings) -> String {
    let label = if account.name.trim().is_empty() {
        account.full_name.as_deref().unwrap_or("Account")
    } else {
        account.name.as_str()
    };
    normalize_initials(label)
}

fn normalize_initials(value: &str) -> String {
    let words: Vec<&str> = value
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let raw = if words.len() >= 2 {
        words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        value
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .take(2)
            .collect()
    };
    let normalized = raw.trim().to_uppercase();
    if normalized.is_empty() {
        "A".to_string()
    } else {
        normalized.chars().take(4).collect()
    }
}

fn account_color_hue(account: &AccountSettings) -> u16 {
    let seed = format!("{}:{}", account.id.as_str(), account.name);
    let hash = seed.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    (hash % 361) as u16
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
    if let Some(appearance) = &request.appearance {
        account.appearance = Some(normalize_account_appearance(appearance.clone()));
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

pub(super) fn normalize_automation_rules(rules: &[AutomationRule]) -> Vec<AutomationRule> {
    rules
        .iter()
        .map(|rule| AutomationRule {
            id: rule.id.trim().to_string(),
            name: rule.name.trim().to_string(),
            enabled: rule.enabled,
            triggers: rule.triggers.clone(),
            condition: rule.condition.clone(),
            actions: rule
                .actions
                .iter()
                .map(normalize_automation_action)
                .collect(),
            backfill: rule.backfill,
        })
        .collect()
}

fn normalize_automation_action(action: &AutomationAction) -> AutomationAction {
    match action {
        AutomationAction::ApplyTag { tag } => AutomationAction::ApplyTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::RemoveTag { tag } => AutomationAction::RemoveTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::MarkRead => AutomationAction::MarkRead,
        AutomationAction::MarkUnread => AutomationAction::MarkUnread,
        AutomationAction::Flag => AutomationAction::Flag,
        AutomationAction::Unflag => AutomationAction::Unflag,
        AutomationAction::MoveToMailbox { mailbox_id } => AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from(mailbox_id.as_str().trim()),
        },
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
