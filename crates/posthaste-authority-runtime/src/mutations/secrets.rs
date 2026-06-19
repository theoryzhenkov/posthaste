use super::*;

impl AccountMutationService {
    pub(super) fn apply_secret_instruction(
        &self,
        account: &mut AccountSettings,
        previous_secret_ref: Option<&SecretRef>,
        secret: &SecretWriteMutation,
    ) -> Result<(), RuntimeError> {
        let decision = decide_secret_instruction(&account.id, previous_secret_ref, secret)?;
        match &decision.store_instruction {
            SecretStoreInstruction::None => {}
            SecretStoreInstruction::Save {
                secret_ref,
                password,
            } => self
                .secret_store
                .save(secret_ref, password)
                .map_err(ServiceError::from)?,
            SecretStoreInstruction::Update {
                secret_ref,
                password,
            } => self
                .secret_store
                .update(secret_ref, password)
                .map_err(ServiceError::from)?,
            SecretStoreInstruction::Delete { secret_ref } => self
                .secret_store
                .delete(secret_ref)
                .map_err(ServiceError::from)?,
        }
        match decision.account_secret_ref {
            AccountSecretRefUpdate::Preserve => {}
            AccountSecretRefUpdate::Set(secret_ref) => account.transport.secret_ref = secret_ref,
        }
        Ok(())
    }

    pub(super) fn delete_managed_secret(
        &self,
        secret_ref: Option<&SecretRef>,
    ) -> Result<(), RuntimeError> {
        if let Some(secret_ref) = secret_ref {
            if matches!(secret_ref.kind, SecretKind::Os) {
                self.secret_store
                    .delete(secret_ref)
                    .map_err(ServiceError::from)?;
            }
        }
        Ok(())
    }
}

pub(super) struct SecretInstructionDecision<'a> {
    account_secret_ref: AccountSecretRefUpdate,
    store_instruction: SecretStoreInstruction<'a>,
}
impl SecretInstructionDecision<'_> {
    pub(super) fn resolved_secret_ref(
        &self,
        previous_secret_ref: Option<&SecretRef>,
    ) -> Option<SecretRef> {
        match &self.account_secret_ref {
            AccountSecretRefUpdate::Preserve => previous_secret_ref.cloned(),
            AccountSecretRefUpdate::Set(secret_ref) => secret_ref.clone(),
        }
    }
}

enum AccountSecretRefUpdate {
    Preserve,
    Set(Option<SecretRef>),
}
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

pub(super) fn decide_secret_instruction<'a>(
    account_id: &AccountId,
    previous_secret_ref: Option<&SecretRef>,
    secret: &'a SecretWriteMutation,
) -> Result<SecretInstructionDecision<'a>, RuntimeError> {
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

fn validate_secret_request(secret: &SecretWriteMutation) -> Result<(), RuntimeError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(RuntimeError::invalid_secret(
                    "secret.password is only allowed when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Replace => {
            required_secret_password(secret)?;
        }
        SecretWriteMode::Clear => {
            if secret.password.is_some() {
                return Err(RuntimeError::invalid_secret(
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

fn required_secret_password(secret: &SecretWriteMutation) -> Result<&str, RuntimeError> {
    secret
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::invalid_secret("secret.password is required when secret.mode is replace")
        })
}

fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}
