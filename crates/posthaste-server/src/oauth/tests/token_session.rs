use super::*;

#[test]
fn token_set_debug_redacts_secret_values() {
    let token_set = OAuthTokenSet {
        r#type: oauth_secret_type(),
        provider: ProviderHint::Gmail,
        client_id: "client-id".to_string(),
        client_secret: Some("client-secret-value".to_string()),
        access_token: "access-token-value".to_string(),
        refresh_token: Some("refresh-token-value".to_string()),
        expires_at: Some("2026-04-27T10:00:00Z".to_string()),
        scopes: vec!["https://mail.google.com/".to_string()],
    };

    let debug = format!("{token_set:?}");

    assert!(!debug.contains("client-secret-value"));
    assert!(!debug.contains("access-token-value"));
    assert!(!debug.contains("refresh-token-value"));
    assert!(debug.contains("[redacted]"));
}

#[test]
fn token_set_refreshes_inside_expiry_skew() {
    let now = OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now");
    let token_set = OAuthTokenSet {
        r#type: oauth_secret_type(),
        provider: ProviderHint::Gmail,
        client_id: "client".to_string(),
        client_secret: Some("secret".to_string()),
        access_token: "access".to_string(),
        refresh_token: Some("refresh".to_string()),
        expires_at: Some(
            (now + Duration::seconds(OAUTH_REFRESH_SKEW_SECONDS - 1))
                .format(&Rfc3339)
                .expect("expiry"),
        ),
        scopes: vec!["https://mail.google.com/".to_string()],
    };

    assert!(token_set.requires_refresh_at(now).expect("refresh check"));
}

#[test]
fn token_set_rejects_wrong_secret_type() {
    let error = OAuthTokenSet::decode(
        r#"{
            "type": "password",
            "provider": "gmail",
            "clientId": "client",
            "accessToken": "access"
        }"#,
    )
    .expect_err("OAuth token secret type is required");

    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("secret type")));
}

#[test]
fn authorization_session_uses_pkce_and_state() {
    let service = OAuthTokenService::new().expect("service");
    let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");

    let session = service
        .authorization_session(
            &profile,
            "client-id",
            Some("client-secret"),
            "http://127.0.0.1:12345/oauth/callback",
        )
        .expect("session");

    assert!(session
        .authorization_url
        .contains("code_challenge_method=S256"));
    assert!(session.authorization_url.contains("access_type=offline"));
    assert!(session.authorization_url.contains("nonce="));
    assert!(!session.state.is_empty());
    assert!(!session.pkce_verifier.is_empty());
    assert!(!session.nonce.is_empty());
}
