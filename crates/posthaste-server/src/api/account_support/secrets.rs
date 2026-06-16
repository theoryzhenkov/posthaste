use super::*;

/// Execute a secret write instruction (keep/replace/clear) against the OS
/// keyring and update the account's `secret_ref` accordingly.
///
/// @spec docs/L1-api#secret-management
#[cfg(test)]
pub(crate) fn apply_secret_instruction(
    secret_store: &dyn posthaste_domain::SecretStore,
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
        } => secret_store
            .save(secret_ref, password)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        SecretStoreInstruction::Update {
            secret_ref,
            password,
        } => secret_store
            .update(secret_ref, password)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        SecretStoreInstruction::Delete { secret_ref } => secret_store
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

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SecretInstructionDecision<'a> {
    pub(crate) account_secret_ref: AccountSecretRefUpdate,
    pub(crate) store_instruction: SecretStoreInstruction<'a>,
}

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum AccountSecretRefUpdate {
    Preserve,
    Set(Option<SecretRef>),
}

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum SecretStoreInstruction<'a> {
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

#[cfg(test)]
pub(crate) fn decide_secret_instruction<'a>(
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
#[cfg(test)]
pub(crate) fn validate_secret_request(secret: &SecretWriteRequest) -> Result<(), ApiError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    ApiErrorCode::InvalidSecret,
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
                    ApiErrorCode::InvalidSecret,
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

/// Extract a non-empty password from the request, returning an error if missing.
#[cfg(test)]
fn required_secret_password(secret: &SecretWriteRequest) -> Result<&str, ApiError> {
    secret
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidSecret,
                "secret.password is required when secret.mode is replace",
            )
        })
}

/// Build the default OS keyring secret reference for an account (`account:{id}`).
#[cfg(test)]
pub(crate) fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}
