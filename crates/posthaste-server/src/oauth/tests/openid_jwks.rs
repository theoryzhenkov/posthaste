use super::*;

#[test]
fn openid_claim_decoding_verifies_signature_with_matching_jwk() {
    let (id_token, jwks) = signed_id_token("test-key", "expected-nonce");

    let claims = decode_verified_openid_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &id_token,
        "test-key",
        &jwks,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .expect("signed token should verify");

    assert_eq!(claims.email.as_deref(), Some("user@example.test"));
}

#[test]
fn openid_claim_decoding_rejects_tampered_signature() {
    let (mut id_token, jwks) = signed_id_token("test-key", "expected-nonce");
    id_token.push('a');

    let error = decode_verified_openid_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &id_token,
        "test-key",
        &jwks,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .expect_err("tampered signature should be rejected");

    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("invalid")));
}

#[test]
fn jwks_cache_duration_uses_cache_control_max_age() {
    let mut headers = oauth2::http::HeaderMap::new();
    headers.insert(
        oauth2::http::header::CACHE_CONTROL,
        oauth2::http::HeaderValue::from_static("public, max-age=120"),
    );

    assert_eq!(jwks_cache_duration(&headers), Duration::seconds(120));
}
