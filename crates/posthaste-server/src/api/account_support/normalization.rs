#[cfg(test)]
use super::*;

/// Trim whitespace from an optional string, converting empty/blank to `None`.
pub(crate) fn normalize_optional(value: Option<String>) -> Option<String> {
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
#[cfg(test)]
pub(crate) fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountRequest) {
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

/// Normalize user-owned email addresses/patterns by trimming whitespace and
/// dropping empty entries. Patterns such as `*@example.com` are preserved.
#[cfg(test)]
pub(crate) fn normalize_email_patterns(patterns: &[String]) -> Vec<String> {
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
