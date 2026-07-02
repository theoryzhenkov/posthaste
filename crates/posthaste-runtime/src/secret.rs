use keyring::{Entry, Error as KeyringError};
use posthaste_domain_model::{SecretKind, SecretRef, SecretStoreError};
use posthaste_domain_service::SecretStore;

/// Keyring service name used for all OS-managed provider secrets.
const KEYRING_SERVICE_NAME: &str = "posthaste";

/// [`SecretStore`] implementation backed by the OS keyring for OS secrets and
/// by environment variables for env secrets.
///
/// The authority runtime owns provider-secret resolution; adapters receive only
/// runtime outputs, never provider credentials.
///
/// spec: docs/runtime/internals/L1#provider-secrets-runtime-store
pub struct SystemSecretStore;

impl SystemSecretStore {
    fn entry(secret_ref: &SecretRef) -> Result<Entry, SecretStoreError> {
        Entry::new(KEYRING_SERVICE_NAME, &secret_ref.key)
            .map_err(|err| SecretStoreError::Unavailable(err.to_string()))
    }
}

impl SecretStore for SystemSecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        match secret_ref.kind {
            SecretKind::Env => std::env::var(&secret_ref.key).map_err(|_| {
                SecretStoreError::Unavailable(format!("environment variable {}", secret_ref.key))
            }),
            SecretKind::Os => Self::entry(secret_ref)?
                .get_password()
                .map_err(|err| SecretStoreError::Unavailable(err.to_string())),
        }
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        match secret_ref.kind {
            SecretKind::Env => Err(SecretStoreError::Unsupported(format!(
                "save via {:?}:{}",
                secret_ref.kind, secret_ref.key
            ))),
            SecretKind::Os => Self::entry(secret_ref)?
                .set_password(value)
                .map_err(|err| SecretStoreError::Unavailable(err.to_string())),
        }
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        match secret_ref.kind {
            SecretKind::Env => Err(SecretStoreError::Unsupported(format!(
                "delete via {:?}:{}",
                secret_ref.kind, secret_ref.key
            ))),
            SecretKind::Os => match Self::entry(secret_ref)?.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
                Err(err) => Err(SecretStoreError::Unavailable(err.to_string())),
            },
        }
    }
}
