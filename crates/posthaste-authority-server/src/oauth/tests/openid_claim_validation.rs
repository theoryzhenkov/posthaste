use super::*;

#[test]
fn openid_claims_require_matching_nonce_and_verified_email() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::json!({
            "aud": "client-id",
            "email": "user@example.test",
            "email_verified": true,
            "exp": 2000000000,
            "iss": "https://accounts.google.com",
            "nonce": "expected-nonce",
        })
        .to_string(),
    );
    let claims = insecure_openid_claims_from_id_token(&format!("header.{payload}.signature"))
        .expect("claims");

    assert_eq!(claims.email.as_deref(), Some("user@example.test"));
    assert_eq!(claims.email_verified, Some(true));
    assert_eq!(claims.nonce.as_deref(), Some("expected-nonce"));
    assert!(validate_openid_identity_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &claims,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .is_ok());
}

#[test]
fn openid_claim_validation_rejects_wrong_audience() {
    let claims = OpenIdTokenClaims {
        aud: Some(OpenIdAudience::One("other-client".to_string())),
        email: Some("user@example.test".to_string()),
        email_verified: Some(true),
        exp: Some(2_000_000_000),
        nbf: None,
        iss: Some("https://accounts.google.com".to_string()),
        preferred_username: None,
        upn: None,
        nonce: Some("expected-nonce".to_string()),
    };

    let error = validate_openid_identity_claims(
        &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
        "client-id",
        &claims,
        "expected-nonce",
        OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
    )
    .expect_err("wrong audience should be rejected");

    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("audience")));
}

#[test]
fn openid_claim_validation_rejects_not_yet_valid_nbf() {
    let base = OpenIdTokenClaims {
        aud: Some(OpenIdAudience::One("client-id".to_string())),
        email: Some("user@example.test".to_string()),
        email_verified: Some(true),
        exp: Some(2_000_000_000),
        nbf: None,
        iss: Some("https://accounts.google.com".to_string()),
        preferred_username: None,
        upn: None,
        nonce: Some("expected-nonce".to_string()),
    };
    let now = OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now");
    let validate = |claims: &OpenIdTokenClaims| {
        validate_openid_identity_claims(
            &OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile"),
            "client-id",
            claims,
            "expected-nonce",
            now,
        )
    };

    // nbf far in the future -> rejected as not yet valid.
    let future = OpenIdTokenClaims {
        nbf: Some(now.unix_timestamp() + 3600),
        ..base.clone()
    };
    let error = validate(&future).expect_err("future nbf should be rejected");
    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("not yet valid")));

    // nbf in the past (and absent) -> accepted.
    let past = OpenIdTokenClaims {
        nbf: Some(now.unix_timestamp() - 3600),
        ..base.clone()
    };
    assert!(validate(&past).is_ok());
    assert!(validate(&base).is_ok());

    // within the clock-skew leeway -> accepted.
    let skewed = OpenIdTokenClaims {
        nbf: Some(now.unix_timestamp() + OPENID_NBF_LEEWAY_SECONDS - 1),
        ..base
    };
    assert!(validate(&skewed).is_ok());
}
