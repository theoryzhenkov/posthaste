use super::*;

/// Validate required fields for an account: non-empty ID and name, plus
/// base URL and configured secret for JMAP accounts.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub(crate) fn validate_account_settings(account: &AccountSettings) -> Result<(), ApiError> {
    if account.id.as_str().trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
            "account id is required",
        ));
    }
    if account.name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
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
            ApiErrorCode::InvalidAccount,
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
                ApiErrorCode::AccountBaseUrlRequired,
                "JMAP base URL is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::AccountSecretRequired,
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
                ApiErrorCode::AccountUsernameRequired,
                "IMAP/SMTP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::AccountSecretRequired,
                "IMAP/SMTP secret must be configured before saving the account",
            ));
        }
        validate_endpoint("IMAP", account.transport.imap.as_ref())?;
        validate_endpoint("SMTP", account.transport.smtp.as_ref())?;
        if !has_concrete_sender_email(account) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::AccountSenderRequired,
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
            ApiErrorCode::InvalidAccount,
            format!("{label} endpoint is required"),
        )
    })?;
    if endpoint.host().trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
            format!("{label} host is required"),
        ));
    }
    if endpoint.port() == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
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

pub(crate) fn validate_automation_rules(rules: &[AutomationRule]) -> Result<(), ApiError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation rule id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation rule ids must be unique",
            ));
        }
        if rule.name.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation rule name is required",
            ));
        }
        if rule.triggers.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation rule must include at least one trigger",
            ));
        }
        if rule.actions.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
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
                        ApiErrorCode::InvalidAccount,
                        "automation tag must be a non-system keyword",
                    ));
                }
                AutomationAction::MoveToMailbox { mailbox_id }
                    if mailbox_id.as_str().trim().is_empty() =>
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        ApiErrorCode::InvalidAccount,
                        "automation target mailbox id is required",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
