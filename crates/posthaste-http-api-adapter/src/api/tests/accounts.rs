use super::*;

#[test]
fn jmap_account_requires_configured_secret() {
    let account = AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: None,
        transport: posthaste_domain_service::AccountTransportSettings {
            base_url: Some("https://example.com/jmap".to_string()),
            username: Some("alice@example.com".to_string()),
            secret_ref: None,
            ..Default::default()
        },
        created_at: "2026-03-31T10:00:00Z".to_string(),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    };

    let error = validate_account_settings(&account).expect_err("validation should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::AccountSecretRequired);
}

#[test]
fn jmap_account_allows_bearer_secret_without_username() {
    let account = AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: None,
        transport: posthaste_domain_service::AccountTransportSettings {
            base_url: Some("https://example.com/jmap".to_string()),
            username: None,
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_JMAP_TOKEN".to_string(),
            }),
            ..Default::default()
        },
        created_at: "2026-03-31T10:00:00Z".to_string(),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    };

    assert!(validate_account_settings(&account).is_ok());
}

#[test]
fn imap_smtp_account_requires_sender_email_pattern() {
    let account = imap_smtp_account("alice-login", vec!["*@example.com"]);

    let error = validate_account_settings(&account).expect_err("validation should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::AccountSenderRequired);
    assert!(error.body.message.contains("sender email"));
}

#[test]
fn imap_smtp_account_allows_username_with_sender_email_pattern() {
    let account = imap_smtp_account("alice-login", vec!["alice@example.com"]);

    assert!(validate_account_settings(&account).is_ok());
}

#[test]
fn imap_smtp_account_rejects_email_username_without_sender_email_pattern() {
    let account = imap_smtp_account("alice@example.com", Vec::new());

    let error = validate_account_settings(&account).expect_err("validation should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::AccountSenderRequired);
    assert!(error.body.message.contains("sender email"));
}

#[test]
fn secret_replace_requires_password() {
    let error = validate_secret_request(&SecretWriteRequest {
        mode: SecretWriteMode::Replace,
        password: None,
    })
    .expect_err("validation should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidSecret);
}

#[test]
fn secret_status_redacts_os_reference() {
    let status = secret_status(Some(&SecretRef {
        kind: SecretKind::Os,
        key: "account:primary".to_string(),
    }));

    assert_eq!(status.storage, SecretStorage::Os);
    assert!(status.configured);
    assert_eq!(status.label, None);
}

#[test]
fn patch_account_preserves_username_when_username_is_omitted() {
    let mut account = AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Jmap,
        enabled: true,
        appearance: None,
        transport: posthaste_domain_service::AccountTransportSettings {
            base_url: Some("https://before.example/jmap".to_string()),
            username: Some("alice@example.com".to_string()),
            secret_ref: None,
            ..Default::default()
        },
        created_at: "2026-03-31T10:00:00Z".to_string(),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    };

    apply_account_patch(
        &mut account,
        &PatchAccountRequest {
            name: None,
            full_name: None,
            signature: None,
            email_patterns: None,
            driver: None,
            enabled: None,
            appearance: None,
            transport: Some(AccountTransportRequest {
                base_url: Some("https://after.example/jmap".to_string()),
                username: None,
                ..Default::default()
            }),
            secret: None,
        },
    );

    assert_eq!(
        account.transport.base_url.as_deref(),
        Some("https://after.example/jmap")
    );
    assert_eq!(
        account.transport.username.as_deref(),
        Some("alice@example.com")
    );
}

#[test]
fn account_appearance_accepts_camel_case_json() {
    let payload = r#"{"kind":"initials","initials":"P","colorHue":245}"#;
    let appearance: AccountAppearance =
        serde_json::from_str(payload).expect("camelCase appearance should deserialize");

    assert_eq!(
        appearance,
        AccountAppearance::Initials {
            initials: "P".to_string(),
            color_hue: 245,
        }
    );
}

#[test]
fn account_transport_request_keeps_provider_hint_json_field() {
    let request: AccountTransportRequest =
        serde_json::from_str(r#"{"provider":"gmail","auth":"oauth2"}"#)
            .expect("legacy provider field should deserialize");
    let transport = posthaste_domain_service::AccountTransportSettings::from(request);

    assert_eq!(transport.provider, ProviderHint::Gmail);
    assert_eq!(
        transport.provider_kind(),
        posthaste_domain_service::ProviderKind::Gmail
    );
    assert_eq!(transport.auth, ProviderAuthKind::OAuth2);
}

fn imap_smtp_account(username: &str, email_patterns: Vec<&str>) -> AccountSettings {
    AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: None,
        signature: None,
        email_patterns: email_patterns.into_iter().map(str::to_string).collect(),
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: posthaste_domain_service::AccountTransportSettings {
            username: Some(username.to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_IMAP_PASSWORD".to_string(),
            }),
            imap: Some(ImapTransportSettings {
                host: "imap.example.com".to_string(),
                port: 993,
                security: posthaste_domain_service::TransportSecurity::Tls,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "smtp.example.com".to_string(),
                port: 587,
                security: posthaste_domain_service::TransportSecurity::StartTls,
            }),
            ..Default::default()
        },
        created_at: "2026-03-31T10:00:00Z".to_string(),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    }
}
