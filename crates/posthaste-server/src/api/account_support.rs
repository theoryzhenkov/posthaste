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
            auth: account.transport.auth.clone(),
            username: account.transport.username.clone(),
            imap: account.transport.imap.clone(),
            smtp: account.transport.smtp.clone(),
            secret,
        },
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            AccountConnectionOverview::ManualCredentials {
                provider: account.transport.provider.clone(),
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
    let decision = decide_secret_instruction(&account.id, previous_secret_ref, secret)?;

    match &decision.store_instruction {
        SecretStoreInstruction::None => {}
        SecretStoreInstruction::Save {
            secret_ref,
            password,
        } => state
            .secret_store
            .save(secret_ref, password)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        SecretStoreInstruction::Update {
            secret_ref,
            password,
        } => state
            .secret_store
            .update(secret_ref, password)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        SecretStoreInstruction::Delete { secret_ref } => state
            .secret_store
            .delete(secret_ref)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
    }

    match decision.account_secret_ref {
        AccountSecretRefUpdate::Preserve => {}
        AccountSecretRefUpdate::Set(secret_ref) => {
            account.transport.secret_ref = secret_ref;
        }
    }

    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct SecretInstructionDecision<'a> {
    account_secret_ref: AccountSecretRefUpdate,
    store_instruction: SecretStoreInstruction<'a>,
}

#[derive(Debug, Eq, PartialEq)]
enum AccountSecretRefUpdate {
    Preserve,
    Set(Option<SecretRef>),
}

#[derive(Debug, Eq, PartialEq)]
enum SecretStoreInstruction<'a> {
    None,
    Save {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Update {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Delete {
        secret_ref: SecretRef,
    },
}

fn decide_secret_instruction<'a>(
    account_id: &AccountId,
    previous_secret_ref: Option<&SecretRef>,
    secret: &'a SecretWriteRequest,
) -> Result<SecretInstructionDecision<'a>, ApiError> {
    validate_secret_request(secret)?;

    let decision = match secret.mode {
        SecretWriteMode::Keep => SecretInstructionDecision {
            account_secret_ref: previous_secret_ref
                .cloned()
                .map(|secret_ref| AccountSecretRefUpdate::Set(Some(secret_ref)))
                .unwrap_or(AccountSecretRefUpdate::Preserve),
            store_instruction: SecretStoreInstruction::None,
        },
        SecretWriteMode::Replace => {
            let password = required_secret_password(secret)?;
            let secret_ref = previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .unwrap_or_else(|| account_secret_ref(account_id));
            let store_instruction = match previous_secret_ref {
                Some(existing) if existing == &secret_ref => SecretStoreInstruction::Update {
                    secret_ref: secret_ref.clone(),
                    password,
                },
                _ => SecretStoreInstruction::Save {
                    secret_ref: secret_ref.clone(),
                    password,
                },
            };
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(secret_ref)),
                store_instruction,
            }
        }
        SecretWriteMode::Clear => SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .map(|secret_ref| SecretStoreInstruction::Delete { secret_ref })
                .unwrap_or(SecretStoreInstruction::None),
        },
    };

    Ok(decision)
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
    if matches!(account.driver, AccountDriver::ImapSmtp) {
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
                "IMAP/SMTP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "IMAP/SMTP secret must be configured before saving the account",
            ));
        }
        validate_endpoint("IMAP", account.transport.imap.as_ref())?;
        validate_endpoint("SMTP", account.transport.smtp.as_ref())?;
        if !has_concrete_sender_email(account) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "IMAP/SMTP accounts require a concrete sender email pattern",
            ));
        }
    }
    if let Some(appearance) = &account.appearance {
        validate_account_appearance(appearance)?;
    }
    Ok(())
}

fn validate_endpoint<T>(label: &str, endpoint: Option<&T>) -> Result<(), ApiError>
where
    T: EndpointLike,
{
    let endpoint = endpoint.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            format!("{label} endpoint is required"),
        )
    })?;
    if endpoint.host().trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            format!("{label} host is required"),
        ));
    }
    if endpoint.port() == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            format!("{label} port must be greater than zero"),
        ));
    }
    Ok(())
}

trait EndpointLike {
    fn host(&self) -> &str;
    fn port(&self) -> u16;
}

impl EndpointLike for ImapTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }

    fn port(&self) -> u16 {
        self.port
    }
}

impl EndpointLike for SmtpTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }

    fn port(&self) -> u16 {
        self.port
    }
}

fn has_concrete_sender_email(account: &AccountSettings) -> bool {
    account
        .email_patterns
        .iter()
        .any(|pattern| is_concrete_email_pattern(pattern))
}

fn is_concrete_email_pattern(pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.contains('*') {
        return false;
    }
    pattern
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
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
pub(super) fn account_secret_ref(account_id: &AccountId) -> SecretRef {
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
        if let Some(provider) = &transport.provider {
            account.transport.provider = provider.clone();
        }
        if let Some(auth) = &transport.auth {
            account.transport.auth = auth.clone();
        }
        if transport.base_url.is_some() {
            account.transport.base_url = normalize_optional(transport.base_url.clone());
        }
        if transport.username.is_some() {
            account.transport.username = normalize_optional(transport.username.clone());
        }
        if transport.imap.is_some() {
            account.transport.imap = transport.imap.clone();
        }
        if transport.smtp.is_some() {
            account.transport.smtp = transport.smtp.clone();
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

pub(super) fn validate_automation_drafts(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Result<(), ApiError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in active_rules {
        ids.insert(rule.id.trim().to_string());
    }
    for rule in draft_rules {
        if rule.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation draft id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "automation rule and draft ids must be unique",
            ));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use posthaste_config::TomlConfigRepository;
    use posthaste_domain::{
        ConfigRepository, MailService, MailStore, SecretStore, SecretStoreError,
    };
    use posthaste_store::DatabaseStore;
    use tokio::sync::broadcast;

    use crate::oauth::OAuthFlowStore;
    use crate::supervisor::AccountSupervisor;

    #[test]
    fn secret_keep_preserves_existing_refs_without_store_instruction() {
        let account_id = AccountId::from("primary");
        let request = secret_request(SecretWriteMode::Keep, None);
        let os_ref = secret_ref(SecretKind::Os, "account:primary");
        let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, None, &request),
                "keep should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Preserve,
                store_instruction: SecretStoreInstruction::None,
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&os_ref), &request),
                "keep should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(os_ref.clone())),
                store_instruction: SecretStoreInstruction::None,
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&env_ref), &request),
                "keep should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(env_ref)),
                store_instruction: SecretStoreInstruction::None,
            }
        );
    }

    #[test]
    fn secret_replace_updates_os_ref_or_saves_new_managed_ref() {
        let account_id = AccountId::from("primary");
        let request = secret_request(SecretWriteMode::Replace, Some("  replacement  "));
        let default_ref = account_secret_ref(&account_id);
        let os_ref = secret_ref(SecretKind::Os, "account:custom");
        let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, None, &request),
                "replace should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(default_ref.clone())),
                store_instruction: SecretStoreInstruction::Save {
                    secret_ref: default_ref.clone(),
                    password: "replacement",
                },
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&os_ref), &request),
                "replace should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(os_ref.clone())),
                store_instruction: SecretStoreInstruction::Update {
                    secret_ref: os_ref,
                    password: "replacement",
                },
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&env_ref), &request),
                "replace should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(default_ref.clone())),
                store_instruction: SecretStoreInstruction::Save {
                    secret_ref: default_ref,
                    password: "replacement",
                },
            }
        );
    }

    #[test]
    fn secret_clear_clears_account_ref_and_deletes_only_os_refs() {
        let account_id = AccountId::from("primary");
        let request = secret_request(SecretWriteMode::Clear, None);
        let os_ref = secret_ref(SecretKind::Os, "account:primary");
        let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, None, &request),
                "clear should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(None),
                store_instruction: SecretStoreInstruction::None,
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&env_ref), &request),
                "clear should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(None),
                store_instruction: SecretStoreInstruction::None,
            }
        );
        assert_eq!(
            expect_decision(
                decide_secret_instruction(&account_id, Some(&os_ref), &request),
                "clear should be valid"
            ),
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(None),
                store_instruction: SecretStoreInstruction::Delete { secret_ref: os_ref },
            }
        );
    }

    #[test]
    fn secret_replace_rejects_missing_or_blank_passwords() {
        for password in [None, Some(""), Some("   ")] {
            let request = secret_request(SecretWriteMode::Replace, password);
            let error = decide_secret_instruction(&AccountId::from("primary"), None, &request)
                .expect_err("replace without a nonblank password should fail");

            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.body.code, "invalid_secret");
            assert_eq!(
                error.body.message,
                "secret.password is required when secret.mode is replace"
            );
        }
    }

    #[test]
    fn apply_secret_instruction_replaces_env_ref_with_managed_os_ref() {
        let test_state = test_app_state();
        let mut account = test_account(Some(secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD")));
        let previous_ref = account.transport.secret_ref.clone();
        let request = secret_request(SecretWriteMode::Replace, Some("  replacement  "));
        let expected_ref = account_secret_ref(&account.id);

        apply_secret_instruction(
            &test_state.state,
            &mut account,
            previous_ref.as_ref(),
            &request,
        )
        .unwrap_or_else(|error| {
            panic!(
                "replace should save the managed secret, got {}: {}",
                error.body.code, error.body.message
            )
        });

        assert_eq!(account.transport.secret_ref, Some(expected_ref.clone()));
        assert_eq!(
            test_state.secret_store.calls(),
            vec![SecretStoreCall::Save(
                expected_ref,
                "replacement".to_string()
            )]
        );
    }

    fn expect_decision<'a>(
        result: Result<SecretInstructionDecision<'a>, ApiError>,
        context: &str,
    ) -> SecretInstructionDecision<'a> {
        result.unwrap_or_else(|error| {
            panic!("{context}, got {}: {}", error.body.code, error.body.message)
        })
    }

    fn secret_request(mode: SecretWriteMode, password: Option<&str>) -> SecretWriteRequest {
        SecretWriteRequest {
            mode,
            password: password.map(str::to_string),
        }
    }

    fn secret_ref(kind: SecretKind, key: &str) -> SecretRef {
        SecretRef {
            kind,
            key: key.to_string(),
        }
    }

    fn test_account(secret_ref: Option<SecretRef>) -> AccountSettings {
        AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: vec!["primary@example.com".to_string()],
            driver: AccountDriver::ImapSmtp,
            enabled: true,
            appearance: None,
            transport: AccountTransportSettings {
                username: Some("primary@example.com".to_string()),
                secret_ref,
                imap: Some(ImapTransportSettings {
                    host: "imap.example.com".to_string(),
                    port: 993,
                    security: posthaste_domain::TransportSecurity::Tls,
                }),
                smtp: Some(SmtpTransportSettings {
                    host: "smtp.example.com".to_string(),
                    port: 587,
                    security: posthaste_domain::TransportSecurity::StartTls,
                }),
                ..Default::default()
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }
    }

    struct TestAppState {
        state: AppState,
        secret_store: Arc<RecordingSecretStore>,
        _root: TestRoot,
    }

    fn test_app_state() -> TestAppState {
        let root = TestRoot(
            std::env::temp_dir().join(format!("posthaste-account-support-{}", Uuid::new_v4())),
        );
        let config: Arc<dyn ConfigRepository> =
            Arc::new(TomlConfigRepository::open(root.0.join("config")).expect("open config repo"));
        let database_store = Arc::new(
            DatabaseStore::open(root.0.join("mail.sqlite"), root.0.join("data"))
                .expect("open database store"),
        );
        let store: Arc<dyn MailStore> = database_store.clone();
        let service = Arc::new(MailService::new(database_store, config));
        let secret_store = Arc::new(RecordingSecretStore::default());
        let secret_store_for_state: Arc<dyn SecretStore> = secret_store.clone();
        let (event_sender, _) = broadcast::channel(1);
        let supervisor = Arc::new(AccountSupervisor::new(
            service.clone(),
            store.clone(),
            secret_store_for_state.clone(),
            event_sender.clone(),
            Duration::from_secs(60),
        ));

        TestAppState {
            state: AppState {
                service,
                store,
                secret_store: secret_store_for_state,
                supervisor,
                event_sender,
                account_logo_root: root.0.join("account-assets").join("logos"),
                oauth_flows: Arc::new(OAuthFlowStore::default()),
            },
            secret_store,
            _root: root,
        }
    }

    struct TestRoot(PathBuf);

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum SecretStoreCall {
        Save(SecretRef, String),
        Update(SecretRef, String),
        Delete(SecretRef),
    }

    #[derive(Default)]
    struct RecordingSecretStore {
        calls: Mutex<Vec<SecretStoreCall>>,
    }

    impl RecordingSecretStore {
        fn calls(&self) -> Vec<SecretStoreCall> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn record(&self, call: SecretStoreCall) {
            self.calls.lock().expect("calls lock").push(call);
        }
    }

    impl SecretStore for RecordingSecretStore {
        fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
            Err(SecretStoreError::Unavailable(
                "test store does not resolve secrets".to_string(),
            ))
        }

        fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
            self.record(SecretStoreCall::Save(secret_ref.clone(), value.to_string()));
            Ok(())
        }

        fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
            self.record(SecretStoreCall::Update(
                secret_ref.clone(),
                value.to_string(),
            ));
            Ok(())
        }

        fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
            self.record(SecretStoreCall::Delete(secret_ref.clone()));
            Ok(())
        }
    }
}
