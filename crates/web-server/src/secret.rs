use keyring::Entry;
use mail_domain::{SecretKind, SecretRef, SecretStore, SecretStoreError};

const KEYRING_SERVICE_NAME: &str = "mail-daemon";

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
            SecretKind::Os => Self::entry(secret_ref)?
                .delete_credential()
                .map_err(|err| SecretStoreError::Unavailable(err.to_string())),
        }
    }
}
