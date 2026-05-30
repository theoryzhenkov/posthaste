//! Macaroon capability tokens: root-key resolution, full-scope minting, and
//! verification. Stage A is the foundation + migration only — the full-scope
//! macaroon (no first-party caveats) replaces the former random per-process
//! token and is accepted on every request exactly as the old token was. Caveat
//! enforcement and the per-route authz map land in Stage B.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use std::path::Path;

use base64::Engine;
use macaroon::{Format, Macaroon, MacaroonKey, Verifier};
use posthaste_domain::{SecretKind, SecretRef, SecretStore};

/// Stable macaroon `location` hint embedded in every minted token. Purely
/// informational (macaroons are verified by the HMAC chain, not the location).
const MACAROON_LOCATION: &str = "posthaste";

/// Keyring account (lookup key) under which the base64 root key is stored. The
/// service name is fixed inside `SystemSecretStore` ("posthaste").
const ROOT_KEY_SECRET_KEY: &str = "macaroon-root-key";

/// Environment variable carrying a base64-encoded 32-byte root key, consulted
/// first so tests/CI/headless runs are deterministic and keyring-free.
const ROOT_KEY_ENV: &str = "POSTHASTE_MACAROON_ROOT_KEY";

/// Length of the HMAC root key in bytes (matches `MacaroonKey`'s primitive).
const ROOT_KEY_LEN: usize = 32;

/// The 32-byte HMAC root key and the `MacaroonKey` derived from it. The raw
/// bytes are kept so the key can be re-persisted; the `MacaroonKey` is what
/// `Macaroon::create` / `Verifier::verify` consume.
#[derive(Clone)]
pub struct RootKey {
    bytes: [u8; ROOT_KEY_LEN],
    key: MacaroonKey,
}

impl RootKey {
    /// Build a root key from exactly 32 raw bytes. The bytes are used verbatim
    /// as the HMAC key (`MacaroonKey::from([u8; 32])`), so the same bytes always
    /// produce verifiable macaroons.
    fn from_bytes(bytes: [u8; ROOT_KEY_LEN]) -> Self {
        let key = MacaroonKey::from(bytes);
        Self { bytes, key }
    }

    /// Construct a deterministic root key from raw bytes, for tests that need to
    /// mint a macaroon and verify it under a shared, keyring-free key.
    pub fn from_test_bytes(bytes: [u8; ROOT_KEY_LEN]) -> Self {
        Self::from_bytes(bytes)
    }

    /// The `MacaroonKey` for minting and verification.
    pub fn macaroon_key(&self) -> &MacaroonKey {
        &self.key
    }

    /// Base64 (standard, padded) encoding of the raw bytes, used for storage.
    fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.bytes)
    }
}

/// Decode a base64 string (accepting standard or URL-safe, padded or not) into
/// exactly 32 bytes, returning `None` on any decode/length mismatch.
fn decode_root_key(encoded: &str) -> Option<[u8; ROOT_KEY_LEN]> {
    let trimmed = encoded.trim();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
        .ok()?;
    decoded.try_into().ok()
}

/// Generate 32 cryptographically-random bytes. Reuses `uuid::Uuid::new_v4`
/// (already in the tree, backed by `getrandom`): two v4 UUIDs supply 32 bytes,
/// of which 122 bits each are random — ample entropy for a per-install HMAC
/// root key.
fn random_root_bytes() -> [u8; ROOT_KEY_LEN] {
    let mut bytes = [0u8; ROOT_KEY_LEN];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes
}

/// The `SecretRef` under which the root key lives in the OS keyring.
fn root_key_secret_ref() -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: ROOT_KEY_SECRET_KEY.to_string(),
    }
}

/// Resolve (or first-run generate + persist) the 32-byte macaroon HMAC root
/// key. Resolution order, designed so the server runs headless where no keyring
/// exists:
///
/// 1. `POSTHASTE_MACAROON_ROOT_KEY` env (base64 of 32 bytes) — tests/CI/headless.
/// 2. OS keyring via `secret_store` under a fixed `SecretRef` (base64).
/// 3. A `0600` state-dir file `<state_root>/macaroon.key` (base64), the fallback
///    when the keyring is unavailable (headless Linux).
///
/// If none exists, generate 32 random bytes and persist them to whichever store
/// is reachable (keyring first, else the file), then use them.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn resolve_root_key(secret_store: &dyn SecretStore, state_root: &Path) -> RootKey {
    // 1. Environment override (deterministic for tests/CI/headless).
    if let Ok(encoded) = std::env::var(ROOT_KEY_ENV) {
        if let Some(bytes) = decode_root_key(&encoded) {
            return RootKey::from_bytes(bytes);
        }
        // A malformed env value is operator error; fall through rather than
        // silently masking it would be surprising, so we keep going to the
        // other stores (the env var simply did not supply a usable key).
    }

    // 2. OS keyring.
    let secret_ref = root_key_secret_ref();
    let keyring_available = match secret_store.resolve(&secret_ref) {
        Ok(encoded) => {
            if let Some(bytes) = decode_root_key(&encoded) {
                return RootKey::from_bytes(bytes);
            }
            // Stored value is corrupt; treat the keyring as writable and
            // overwrite it below with a freshly generated key.
            true
        }
        // `Unavailable` covers both "no entry yet" (keyring works, key absent)
        // and "no keyring at all" (headless). We cannot distinguish them from
        // the error, so we probe by attempting a save after generating.
        Err(_) => true,
    };

    // 3. State-dir file fallback.
    let key_file = state_root.join("macaroon.key");
    if let Ok(encoded) = std::fs::read_to_string(&key_file) {
        if let Some(bytes) = decode_root_key(&encoded) {
            return RootKey::from_bytes(bytes);
        }
    }

    // None found: generate, persist to the first store that accepts it, use it.
    let root = RootKey::from_bytes(random_root_bytes());
    let encoded = root.to_base64();

    if keyring_available && secret_store.save(&secret_ref, &encoded).is_ok() {
        return root;
    }

    // Keyring unavailable (headless): persist to the 0600 state-dir file.
    let _ = std::fs::create_dir_all(state_root);
    if let Err(error) = crate::write_secure_file(&key_file, encoded.as_bytes()) {
        // Persisting failed (read-only state dir, etc.); the in-memory key is
        // still usable for this process, but tokens won't verify after a
        // restart. Surface it rather than failing startup.
        eprintln!(
            "failed to persist macaroon root key at {}: {error}",
            key_file.display()
        );
    }
    root
}

/// Mint a **full-scope** macaroon: signed with the root key, carrying NO
/// first-party caveats, so it is accepted on every request (Stage A behavior
/// parity with the former random token). The identifier is a random UUID,
/// giving each process a distinct token. Returned as the V2 serialization,
/// which `macaroon` emits as a URL-safe base64 ASCII string — directly usable
/// as the `Authorization: Bearer` value.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn mint_full_scope_token(root: &RootKey) -> String {
    let identifier = uuid::Uuid::new_v4().to_string();
    let macaroon = Macaroon::create(
        Some(MACAROON_LOCATION.to_string()),
        root.macaroon_key(),
        identifier.into(),
    )
    .expect("full-scope macaroon should mint (non-empty identifier)");
    macaroon
        .serialize(Format::V2)
        .expect("V2 macaroon serialization should not fail")
}

/// Verify a presented bearer token as a macaroon signed by `root`. Stage A: the
/// verifier satisfies NO first-party caveats, so a full-scope macaroon (no
/// caveats) passes and any caveat-bearing macaroon fails (acceptable — only
/// full-scope tokens exist until Stage B adds caveat enforcement). Returns
/// `true` iff the token deserializes and its HMAC chain verifies against the
/// root key.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn verify_token(presented: &str, root: &RootKey) -> bool {
    let Ok(macaroon) = Macaroon::deserialize(presented) else {
        return false;
    };
    let verifier = Verifier::default();
    verifier
        .verify(&macaroon, root.macaroon_key(), Vec::new())
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed 32-byte test key, base64-encoded, so tests are deterministic.
    fn test_root_key() -> RootKey {
        RootKey::from_bytes([7u8; ROOT_KEY_LEN])
    }

    #[test]
    fn full_scope_token_verifies_against_its_root_key() {
        let root = test_root_key();
        let token = mint_full_scope_token(&root);
        assert!(verify_token(&token, &root));
    }

    #[test]
    fn garbage_token_fails_verification() {
        let root = test_root_key();
        assert!(!verify_token("not-a-macaroon", &root));
        assert!(!verify_token("", &root));
    }

    #[test]
    fn token_from_different_root_key_fails() {
        let root_a = test_root_key();
        let root_b = RootKey::from_bytes([9u8; ROOT_KEY_LEN]);
        let token = mint_full_scope_token(&root_a);
        assert!(!verify_token(&token, &root_b));
    }

    #[test]
    fn decode_root_key_accepts_standard_and_url_safe() {
        let bytes = [3u8; ROOT_KEY_LEN];
        let std_b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let url_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        assert_eq!(decode_root_key(&std_b64), Some(bytes));
        assert_eq!(decode_root_key(&url_b64), Some(bytes));
        assert_eq!(decode_root_key("too short"), None);
    }

    #[test]
    fn round_trip_token_string_is_ascii() {
        // The serialized macaroon must be a header-safe ASCII string (it goes in
        // an Authorization header and into daemon.json verbatim).
        let token = mint_full_scope_token(&test_root_key());
        assert!(token.is_ascii());
        assert!(!token.contains(' '));
    }
}
