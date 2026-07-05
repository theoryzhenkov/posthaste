use std::sync::Mutex;

use keyring::{Entry, Error as KeyringError};
use posthaste_domain_model::{SecretKind, SecretRef, SecretStoreError};
use posthaste_domain_service::{SecretCasOutcome, SecretStore};

/// Keyring service name used for all OS-managed provider secrets.
const KEYRING_SERVICE_NAME: &str = "posthaste";

/// Process-wide guard serializing [`SystemSecretStore::update_if_unchanged`]
/// read-compare-write sequences (D101 / A1).
///
/// The keyring and env backings expose no native compare-and-swap, so the CAS is
/// a read → compare → write. This lock makes that sequence atomic **within this
/// process** — two in-process OAuth refreshes racing the same secret ref can no
/// longer interleave their read/write and last-writer-wins a consumed refresh
/// token. Residual (documented, accepted): the lock does **not** span processes;
/// two distinct posthaste processes sharing one OS keyring can still both observe
/// `expected_current`, compare-true, and write (a cross-process TOCTOU). In this
/// single-process authority server that window does not arise, and the M34
/// per-secret-ref refresh single-flight already serializes same-ref refreshes
/// upstream — this CAS is the defense-in-depth backstop that also *detects* a
/// drifted store (returning the winner's value) rather than silently clobbering.
static CAS_GUARD: Mutex<()> = Mutex::new(());

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

    fn update_if_unchanged(
        &self,
        secret_ref: &SecretRef,
        expected_current: &str,
        new_value: &str,
    ) -> Result<SecretCasOutcome, SecretStoreError> {
        // Serialize the read-compare-write against other in-process CAS callers;
        // the keyring/env backing has no native atomic CAS (see `CAS_GUARD`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_service::SecretCasOutcome;

    fn env_ref(key: &str) -> SecretRef {
        SecretRef {
            kind: SecretKind::Env,
            key: key.to_string(),
        }
    }

    /// D101 / A1: the CAS override rejects a write whose `expected_current` has
    /// drifted from the stored value (a concurrent writer rotated it), returning
    /// the winner's value so the caller adopts it instead of clobbering. Exercised
    /// over the env backing, whose `resolve` reads the current value — the
    /// Mismatch arm is the clobber-preventing path A1 turns on.
    #[test]
    fn cas_rejects_stale_expected_and_returns_current() {
        let key = "POSTHASTE_CAS_TEST_STALE";
        std::env::set_var(key, "winner-value");

        let outcome = SystemSecretStore
            .update_if_unchanged(&env_ref(key), "stale-expected", "loser-value")
            .expect("cas resolves the current env value");

        assert_eq!(
            outcome,
            SecretCasOutcome::Mismatch {
                current: "winner-value".to_string()
            },
            "a stale expected value must miss and carry the winner's value",
        );
        std::env::remove_var(key);
    }

    /// When `expected_current` matches, the CAS proceeds to the backing write
    /// (rather than early-returning Mismatch). The env backing cannot persist, so
    /// the write surfaces `Unsupported` — proving the compare matched and the
    /// swap was attempted, i.e. a fresh (non-stale) refresh is not spuriously
    /// rejected.
    #[test]
    fn cas_matching_expected_attempts_the_write() {
        let key = "POSTHASTE_CAS_TEST_MATCH";
        std::env::set_var(key, "current-value");

        let result =
            SystemSecretStore.update_if_unchanged(&env_ref(key), "current-value", "next-value");

        assert!(
            matches!(result, Err(SecretStoreError::Unsupported(_))),
            "a matching CAS must attempt the backing write, not miss: {result:?}",
        );
        std::env::remove_var(key);
    }
}
