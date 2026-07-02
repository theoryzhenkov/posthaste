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
    let scoped = attenuate(&scoped, "account = acct-a").expect("second attenuation should succeed");

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
