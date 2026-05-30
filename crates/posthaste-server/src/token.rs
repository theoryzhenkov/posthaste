//! Macaroon capability tokens: root-key resolution, full-scope minting, and
//! verification. Stage A is the foundation + migration only — the full-scope
//! macaroon (no first-party caveats) replaces the former random per-process
//! token and is accepted on every request exactly as the old token was. Caveat
//! enforcement and the per-route authz map land in Stage B.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use std::path::Path;

use base64::Engine;
use macaroon::{ByteString, Caveat, Format, Macaroon, MacaroonKey, Verifier};
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

/// Why a presented token failed authenticity verification. Both map to **401**
/// (the credential is forged/garbled); they are distinguished only for clarity.
#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The string did not deserialize as a macaroon.
    Malformed,
    /// The macaroon deserialized but its HMAC chain did not verify against the
    /// root key (forged, or minted under a different key).
    BadSignature,
}

/// Verify a presented bearer token's **authenticity** (HMAC chain) against
/// `root` and, on success, return its first-party caveats for separate
/// evaluation. This deliberately verifies signature ONLY: the verifier is given
/// a general predicate that satisfies every first-party caveat, so a
/// caveat-bearing (attenuated) macaroon still passes the signature check. Caveat
/// ENFORCEMENT happens afterward in [`crate::authz::evaluate`], which yields a
/// 403 (authentic but out of scope) — distinct from the 401 this returns for a
/// forged/garbled token.
///
/// A full-scope macaroon (no caveats) returns `Ok(vec![])` and is allowed
/// everywhere by the (empty) caveat evaluation, exactly as before.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn verify_authenticity(presented: &str, root: &RootKey) -> Result<Vec<Caveat>, TokenError> {
    let macaroon = Macaroon::deserialize(presented).map_err(|_| TokenError::Malformed)?;
    let mut verifier = Verifier::default();
    // Satisfy ALL first-party caveats so `verify` checks the signature chain
    // only; the caveats are returned for independent enforcement.
    fn satisfy_all(_predicate: &ByteString) -> bool {
        true
    }
    verifier.satisfy_general(satisfy_all);
    verifier
        .verify(&macaroon, root.macaroon_key(), Vec::new())
        .map_err(|_| TokenError::BadSignature)?;
    Ok(macaroon.first_party_caveats())
}

/// Stage-A authenticity helper retained for callers that only need a yes/no on
/// the signature (no caveat enforcement). `true` iff the token is authentic
/// under `root`. Stage B enforcement uses [`verify_authenticity`] +
/// [`crate::authz::evaluate`] instead.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn verify_token(presented: &str, root: &RootKey) -> bool {
    verify_authenticity(presented, root).is_ok()
}

/// Append a first-party caveat to a serialized macaroon and re-serialize it.
/// Attenuation is **client-side**: it needs no root key (the caveat is folded
/// into the HMAC chain using the macaroon's own running signature), and it can
/// only NARROW authority. Used by the `posthaste token attenuate` CLI to derive
/// scoped tokens. The predicate must use the documented caveat format (see
/// [`crate::authz`]). Returns the attenuated V2 macaroon string, or an error if
/// the input does not deserialize.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn attenuate(presented: &str, predicate: &str) -> Result<String, TokenError> {
    let mut macaroon = Macaroon::deserialize(presented).map_err(|_| TokenError::Malformed)?;
    macaroon.add_first_party_caveat(predicate.into());
    macaroon
        .serialize(Format::V2)
        .map_err(|_| TokenError::Malformed)
}

/// Mint a macaroon carrying the given first-party caveats, signed by `root`.
/// Equivalent to [`mint_full_scope_token`] followed by repeated [`attenuate`],
/// but in one step against the root key. Used by tests to build scoped tokens.
///
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub fn mint_with_caveats(root: &RootKey, predicates: &[&str]) -> String {
    let identifier = uuid::Uuid::new_v4().to_string();
    let mut macaroon = Macaroon::create(
        Some(MACAROON_LOCATION.to_string()),
        root.macaroon_key(),
        identifier.into(),
    )
    .expect("macaroon should mint (non-empty identifier)");
    for predicate in predicates {
        macaroon.add_first_party_caveat((*predicate).into());
    }
    macaroon
        .serialize(Format::V2)
        .expect("V2 macaroon serialization should not fail")
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
    fn verify_authenticity_returns_caveats_for_attenuated_token() {
        let root = test_root_key();
        let full = mint_full_scope_token(&root);
        let scoped = attenuate(&full, "action = read").expect("attenuation should succeed");
        let scoped =
            attenuate(&scoped, "account = acct-a").expect("second attenuation should succeed");

        // Authentic under the same root key, returning both caveats.
        let caveats = verify_authenticity(&scoped, &root).expect("scoped token is authentic");
        assert_eq!(caveats.len(), 2);

        // A full-scope token yields no caveats.
        let none = verify_authenticity(&full, &root).expect("full-scope authentic");
        assert!(none.is_empty());
    }

    #[test]
    fn attenuated_token_still_fails_under_wrong_root_key() {
        let root_a = test_root_key();
        let root_b = RootKey::from_bytes([9u8; ROOT_KEY_LEN]);
        let scoped = attenuate(&mint_full_scope_token(&root_a), "action = read").unwrap();
        assert_eq!(
            verify_authenticity(&scoped, &root_b),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn malformed_token_reports_malformed() {
        let root = test_root_key();
        assert_eq!(
            verify_authenticity("not-a-macaroon", &root),
            Err(TokenError::Malformed)
        );
    }

    #[test]
    fn mint_with_caveats_matches_attenuation() {
        let root = test_root_key();
        let token = mint_with_caveats(&root, &["action = read", "message = m1"]);
        let caveats = verify_authenticity(&token, &root).expect("authentic");
        assert_eq!(caveats.len(), 2);
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
