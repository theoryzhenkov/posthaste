//! OS-backed secret storage: the [`SecretStore`] port implemented over the
//! OS keyring for `os` secret refs and environment variables for `env` refs.
//! Provider credentials never live in config files or the database.

use std::sync::Mutex;

use keyring::{Entry, Error as KeyringError};
use posthaste_domain_model::{SecretKind, SecretRef, SecretStoreError};
use posthaste_domain_service::{SecretCasOutcome, SecretStore};

/// Keyring service name used for all OS-managed provider secrets.
///
/// FROZEN: every existing install's keychain entries live under this service
/// name (with the secret-ref key as the account name — see
/// [`keyring_entry_location`]), so changing either strands stored credentials.
const KEYRING_SERVICE_NAME: &str = "posthaste";

/// The `(service, account)` pair under which an OS secret ref lives in the
/// OS keyring. This naming is the credential-continuity contract: the
/// secret-ref keys persisted in `sources/*.toml` must keep resolving to the
/// entries earlier releases wrote.
pub fn keyring_entry_location(secret_ref: &SecretRef) -> (&'static str, &str) {
    (KEYRING_SERVICE_NAME, secret_ref.key.as_str())
}

/// Process-wide guard serializing [`SystemSecretStore::update_if_unchanged`]
/// read-compare-write sequences. The keyring and env backings expose no
/// native compare-and-swap, so the CAS is a read → compare → write made
/// atomic within this process by this lock. Residual (documented, accepted):
/// the lock does not span processes; a second posthaste process sharing the
/// OS keyring can still race the window. The CAS remains the defense-in-depth
/// backstop that detects a drifted store (returning the winner's value)
/// rather than silently clobbering it.
static CAS_GUARD: Mutex<()> = Mutex::new(());

/// [`SecretStore`] implementation backed by the OS keyring for OS secrets
/// and by environment variables for env secrets.
pub struct SystemSecretStore;

impl SystemSecretStore {
    fn entry(secret_ref: &SecretRef) -> Result<Entry, SecretStoreError> {
        let (service, account) = keyring_entry_location(secret_ref);
        Entry::new(service, account).map_err(|err| SecretStoreError::Unavailable(err.to_string()))
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

    fn update_if_unchanged(
        &self,
        secret_ref: &SecretRef,
        expected_current: &str,
        new_value: &str,
    ) -> Result<SecretCasOutcome, SecretStoreError> {
        // Serialize the read-compare-write against other in-process CAS
        // callers; the keyring/env backing has no native atomic CAS.
        let _guard = CAS_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = self.resolve(secret_ref)?;
        if current != expected_current {
            return Ok(SecretCasOutcome::Mismatch { current });
        }
        self.update(secret_ref, new_value)?;
        Ok(SecretCasOutcome::Swapped)
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
