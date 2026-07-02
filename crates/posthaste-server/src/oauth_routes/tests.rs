//! Tests for the OAuth provider-flow account construction (the far half), moved
//! here from the api crate's account tests when account creation from a provider
//! exchange became an authority server operation.

use axum::response::IntoResponse;
use posthaste_domain_service::{
    AccountDriver, AccountId, ProviderAuthKind, ProviderHint, SecretKind, SecretRef,
    TransportSecurity,
};

use super::support::{oauth_account_settings, oauth_provider_mail_transport};

#[test]
fn provider_oauth_account_uses_identity_for_username_and_sender_address() {
    let account = match oauth_account_settings(
        AccountId::from("user-example-com"),
        ProviderHint::Gmail,
        "user@example.com".to_string(),
        "user@example.com".to_string(),
        vec!["user@example.com".to_string()],
        SecretRef {
            kind: SecretKind::Os,
            key: "account:user-example-com".to_string(),
        },
        "2026-04-27T10:00:00Z".to_string(),
    ) {
        Ok(account) => account,
        Err(error) => panic!(
            "OAuth account settings should build, got {}",
            error.into_response().status()
        ),
    };

    assert_eq!(account.driver, AccountDriver::ImapSmtp);
    assert_eq!(account.transport.auth, ProviderAuthKind::OAuth2);
    assert_eq!(
        account.transport.username.as_deref(),
        Some("user@example.com")
    );
    assert_eq!(account.email_patterns, vec!["user@example.com"]);
}

#[test]
fn provider_oauth_account_sets_known_mail_endpoints() {
    let (gmail_imap, gmail_smtp) = match oauth_provider_mail_transport(&ProviderHint::Gmail) {
        Ok(transport) => transport,
        Err(error) => panic!(
            "Gmail transport should build, got {}",
            error.into_response().status()
        ),
    };
    let (outlook_imap, outlook_smtp) = match oauth_provider_mail_transport(&ProviderHint::Outlook) {
        Ok(transport) => transport,
        Err(error) => panic!(
            "Outlook transport should build, got {}",
            error.into_response().status()
        ),
    };

    assert_eq!(gmail_imap.host, "imap.gmail.com");
    assert_eq!(gmail_imap.security, TransportSecurity::Tls);
    assert_eq!(gmail_smtp.host, "smtp.gmail.com");
    assert_eq!(gmail_smtp.security, TransportSecurity::StartTls);
    assert_eq!(outlook_imap.host, "outlook.office365.com");
    assert_eq!(outlook_smtp.host, "smtp.office365.com");
}
