use super::*;

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
